use async_recursion::async_recursion;
use clap::{Args};
use serde::{Serialize,Deserialize};
use reqwest::Client;
use derive_more::From;
use tiktoken_rs::p50k_base;
use crate::openai::chat::OpenAIChatCommand;
use crate::openai::OpenAIError;
use crate::completion::{CompletionOptions,CompletionFile,ClashingArgumentsError};
use crate::Config;

const CHAT_TOKENS_MAX: usize = 4096;

#[derive(Args, Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatCommand {
    #[command(flatten)]
    #[serde(flatten)]
    pub completion: CompletionOptions,

    #[arg(long, short)]
    pub system: Option<String>,

    #[arg(long, short)]
    pub direction: Option<String>,
}

impl ChatCommand {
    #[async_recursion]
    pub async fn run(&self, client: &Client, config: &Config) -> ChatResult {
        let mut options = ChatOptions::try_from((self, config))?;
        let print_output = !options.completion.quiet.unwrap_or(false);

        if print_output && options.file.transcript.len() > 0 {
            print!("{}", options.file.transcript);
        }

        if !options.ai_responds_first {
            let append = options.completion.append.as_ref().map(|a| &**a);
            let prefix_user = Some(&*options.prefix_user);

            if let None = options.file.read(append, prefix_user, options.no_context) {
                return Ok(vec![]);
            }
        }

        let mut command = OpenAIChatCommand::try_from(options)?;
        command.run(client, config).await
    }
}

#[derive(Default, Debug)]
pub(crate) struct ChatOptions {
    pub ai_responds_first: bool,
    pub completion: CompletionOptions,
    pub direction: Option<ChatMessage>,
    pub system: String,
    pub file: CompletionFile<ChatCommand>,
    pub no_context: bool,
    pub prefix_ai: String,
    pub prefix_user: String,
    pub stream: bool,
    pub temperature: f32,
    pub tokens_max: usize,
    pub tokens_balance: f32
}

impl TryFrom<(&ChatCommand, &Config)> for ChatOptions {
    type Error = ChatError;

    fn try_from((command, config): (&ChatCommand, &Config)) -> Result<Self, Self::Error> {
        let file = command.completion.load_session_file::<ChatCommand>(config, command.clone());
        let completion = if file.file.is_some() {
            command.completion.merge(&file.overrides.completion)
        } else {
            command.completion.clone()
        };

        let stream = completion.parse_stream_option()?;
        let system = command.system
            .clone()
            .or_else(|| file.overrides.system.clone())
            .clone()
            .unwrap_or_else(|| String::from("A friendly and helpful AI assistant."));

        Ok(ChatOptions {
            ai_responds_first: completion.ai_responds_first.unwrap_or(false),
            direction: command.direction.clone()
                .map(|direction| ChatMessage::new(ChatRole::System, direction)),
            temperature: completion.temperature.unwrap_or(0.8),
            no_context: completion.no_context.unwrap_or(false),
            prefix_ai: completion.prefix_ai.clone().unwrap_or_else(|| String::from("AI")),
            prefix_user: completion.prefix_user.clone().unwrap_or_else(|| String::from("USER")),
            system,
            tokens_balance: completion.tokens_balance.unwrap_or(0.5),
            tokens_max: CHAT_TOKENS_MAX,
            completion,
            stream,
            file,
        })
    }
}

#[derive(Debug, From)]
pub enum ChatError {
    ClashingArguments(ClashingArgumentsError),
    ChatTranscriptionError(ChatTranscriptionError),
    TranscriptDeserializationError(serde_json::Error),
    OpenAIError(OpenAIError),
    NetworkError(reqwest::Error),
    IOError(std::io::Error),
    EventSource(reqwest_eventsource::Error),
    Unauthorized
}

#[derive(Debug)]
pub struct ChatTranscriptionError(pub String);

pub type ChatResult = Result<Vec<ChatMessage>, ChatError>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    #[serde(skip)]
    pub tokens: usize
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl AsRef<str>) -> Self {
        let tokens = p50k_base().unwrap()
            .encode_with_special_tokens(&format!("{}{}", role, content.as_ref()))
            .len();

        ChatMessage {
            role,
            content: content.as_ref().to_string(),
            tokens
        }
    }
}

pub type ChatMessages = Vec<ChatMessage>;

impl TryFrom<&ChatOptions> for ChatMessages {
    type Error = ChatError;

    fn try_from(options: &ChatOptions) -> Result<Self, Self::Error> {
        let ChatOptions { file, system, .. } = options;

        let mut messages = vec![];
        let mut message: Option<ChatMessage> = None;

        messages.push(ChatMessage::new(ChatRole::System, system));

        let handle_continuing_line = |line, message: &mut Option<ChatMessage>| match message {
            Some(m) => {
                *message = Some(ChatMessage::new(m.role, {
                    let mut content = m.content.clone();
                    content += "\n";
                    content += line;
                    content
                }));
                Ok(())
            },
            None => {
                return Err(ChatError::ChatTranscriptionError(ChatTranscriptionError(
                    "Missing opening chat role".into()
                )));
            }
        };

        for line in file.transcript.lines() {
            match line.split_once(':') {
                Some((role, dialog)) => match ChatRole::try_from((role, options)) {
                    Ok(normalized_role) => {
                        if let Some(message) = message {
                            messages.push(message);
                        }

                        let mut dialog = dialog.trim_start().to_string();
                        if role != "ai" && role != "assitant" && role != "user" && role != "system"
                            && !dialog.to_lowercase().starts_with(role) {
                            dialog = format!("{role}: {dialog}");
                        }

                        message = Some(ChatMessage::new(normalized_role, dialog));
                    },
                    Err(_) => handle_continuing_line(line, &mut message)?
                },
                None => handle_continuing_line(line, &mut message)?
            }
        }

        if let Some(message) = message {
            messages.push(message);
        }

        if options.no_context {
            messages.push(ChatMessage::new(ChatRole::User, file.last_read_input.clone()));
        }

        if let Some(direction) = &options.direction {
            messages.push(direction.clone());
        }

        if options.no_context {
            messages.push(ChatMessage::new(ChatRole::Ai, file.last_written_input.clone()))
        }

        let lab = messages.labotomize(&options)?;
        return Ok(lab);
    }
}

pub(crate) trait ChatMessagesInternalExt {
    fn labotomize(&self, options: &ChatOptions) -> Result<Self, ChatError> where Self: Sized;
}

impl ChatMessagesInternalExt for ChatMessages {
    fn labotomize(&self, options: &ChatOptions) -> Result<Self, ChatError> {
        let tokens_max = options.tokens_max;
        let tokens_balance = options.tokens_balance;
        let upper_bound = (tokens_max as f32 * tokens_balance).floor() as usize;
        let current_token_length: usize = self.iter().map(|m| m.tokens).sum();

        if current_token_length > upper_bound {
            let system = ChatMessage::new(ChatRole::System, options.system.clone());
            let mut messages = vec![];
            let mut remaining = upper_bound.checked_sub(system.tokens)
                .ok_or_else(|| ChatTranscriptionError(format!(
                    "Cannot fit your system message into the chat messages list. This means \
                    that your tokens_max value is either too small or your system message is \
                    too long. You're upper bound on transcript tokens is {upper_bound} and \
                    your system message has {} tokens", system.tokens)))?;

            for message in self.iter().skip(1).rev() {
                match remaining.checked_sub(message.tokens) {
                    Some(subtracted) => {
                        remaining = subtracted;
                        messages.push(message);
                    },
                    None => break,
                }
            }

            messages.push(&system);
            Ok(messages.iter().rev().map(|i| i.clone()).cloned().collect())
        } else {
            Ok(self.clone())
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum ChatRole {
    #[serde(rename = "assistant")]
    Ai,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "system")]
    System
}

impl std::fmt::Display for ChatRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", match self {
            Self::Ai => "AI: ",
            Self::User => "USER: ",
            Self::System => "SYSTEM: "
        })
    }
}

impl TryFrom<(&str, &ChatOptions)> for ChatRole {
    type Error = ChatError;

    fn try_from((role, options): (&str, &ChatOptions)) -> Result<Self, Self::Error> {
        let role = role.to_lowercase();
        let role = role.trim();

        if role == options.prefix_ai.to_lowercase() {
            return Ok(ChatRole::Ai)
        }

        if role == options.prefix_user.to_lowercase() {
            return Ok(ChatRole::User)
        }

        match &*role {
            "ai" |
            "assistant" => Ok(ChatRole::Ai),
            "system" => Ok(ChatRole::System),
            _ => Ok(ChatRole::User),
        }
    }
}
