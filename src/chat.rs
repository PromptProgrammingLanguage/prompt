use std::io::{self,Write};
use async_recursion::async_recursion;
use clap::{Args};
use serde::{Serialize,Deserialize};
use reqwest::Client;
use reqwest_eventsource::{EventSource,Event};
use derive_more::From;
use serde_json::json;
use futures_util::stream::StreamExt;
use crate::openai::response::OpenAICompletionResponse;
use crate::openai::OpenAIError;
use crate::completion::{CompletionOptions,CompletionFile};
use crate::Config;

use tiktoken_rs::p50k_base;

const CHAT_TOKENS_MAX: usize = 4096;

#[derive(Args, Clone, Debug, Serialize, Deserialize)]
pub struct ChatCommand {
    #[command(flatten)]
    pub completion: CompletionOptions,

    #[arg(long, short)]
    pub system: Option<String>
}

impl ChatCommand {
    #[async_recursion]
    pub async fn run(&self, client: &Client, config: &Config) -> ChatResult {
        let mut options = ChatOptions::try_from((&*self, config))?;
        let print_output = !options.completion.quiet.unwrap_or(false);

        if print_output && options.file.transcript.len() > 0 {
            println!("{}", options.file.transcript);
        }

        if !options.ai_responds_first {
            let append = options.completion.append.as_ref().map(|a| &**a);

            if let None = options.file.read(append, Some(&*options.prefix_user)) {
                return Ok(vec![]);
            }
        }

        loop {
            if options.stream {
                let result = handle_stream(client, &mut options).await?;
                if result.len() > 0 {
                    return Ok(result);
                }
            } else {
                let result = handle_sync(client, &mut options, print_output).await?;
                if result.len() > 0 {
                    return Ok(result);
                }
            }

            if let None = options.file.read(None, Some(&*options.prefix_user)) {
                return Ok(vec![]);
            }
        }
    }
}

async fn handle_sync(client: &Client, options: &mut ChatOptions, print_output: bool) -> ChatResult {
    let post = client.post("https://api.openai.com/v1/chat/completions");

    let messages = ChatMessages::try_from(&*options)?;
    let request = post
        .json(&json!({
            "model": "gpt-3.5-turbo",
            "temperature": options.temperature,
            "messages": messages,
        }))
        .send()
        .await
        .expect("Failed to send chat");

    if !request.status().is_success() {
        return Err(ChatError::OpenAIError(request.json().await?));
    }

    let chat_response: OpenAICompletionResponse<OpenAIChatChoice> = request.json().await?;
    let text = chat_response.choices.first().unwrap().message
        .as_ref()
        .map(|message| {
            let message = message.content.trim();

            if message.starts_with(&options.prefix_ai) {
                message.to_string()
            } else {
                format!("{}: {}", options.prefix_ai, message)
            }
        });

    if let Some(text) = text {
        let text = options.file.write(text)?;

        if print_output {
            println!("{}", text);
        }

        if options.completion.append.is_some() {
            return Ok(vec![ text ]);
        }
    }

    Ok(vec![])
}

async fn handle_stream(client: &Client, options: &mut ChatOptions) -> ChatResult {
    let post = client.post("https://api.openai.com/v1/chat/completions")
        .json(&json!({
            "model": "gpt-3.5-turbo",
            "temperature": options.temperature,
            "stream": true,
            "messages": ChatMessages::try_from(&*options)?
        }));

    let mut stream = EventSource::new(post).unwrap();
    let mut has_written = false;

    'stream: while let Some(event) = stream.next().await {
        match event {
            Ok(Event::Open) => {},
            Ok(Event::Message(message)) if message.data == "[DONE]" => {
                break 'stream;
            },
            Ok(Event::Message(message)) => {
                let chat_response: OpenAICompletionResponse<OpenAIChatDelta> =
                    serde_json::from_str(&message.data)?;

                let delta = &chat_response.choices.first().unwrap().delta;
                if let Some(ref role) = delta.role {
                    let role = options.file.write_words(format!("{}", role))?;
                    print!("{}", role);
                    has_written = true;
                }
                if let Some(content) = delta.content.clone() {
                    let content = options.file.write_words(content)?;
                    print!("{}", content);
                    has_written = true;
                }
                io::stdout().flush().unwrap();
            }
            Err(err) => {
                stream.close();
                return Err(ChatError::EventSource(err));
            }
        }
    }

    if has_written {
        println!("");
        options.file.write_words(String::from("\n"))?;
        io::stdout().flush().unwrap();
    }

    Ok(vec![])
}

#[derive(Default)]
struct ChatOptions {
    ai_responds_first: bool,
    completion: CompletionOptions,
    system: String,
    file: CompletionFile,
    prefix_ai: String,
    prefix_user: String,
    stream: bool,
    temperature: f32,
    tokens_max: usize,
    tokens_balance: f32
}

impl TryFrom<(&ChatCommand, &Config)> for ChatOptions {
    type Error = ChatError;

    fn try_from((command, config): (&ChatCommand, &Config)) -> Result<Self, Self::Error> {
        let file = command.completion.load_session_file::<CompletionOptions>(config);
        let completion = if file.file.is_some() {
            command.completion.merge(&file.overrides)
        } else {
            command.completion.clone()
        };
        let stream = match (completion.quiet, completion.stream) {
            (Some(true), Some(true)) => return Err(ChatError::ClashingArguments {
                error: "Having both quiet and stream enabled doesn't make sense.".into()
            }),
            (Some(true), None) |
            (Some(true), Some(false)) |
            (None, Some(false)) |
            (Some(false), Some(false)) => false,
            (Some(false), None) |
            (Some(false), Some(true)) |
            (None, Some(true)) |
            (None, None) => true
        };

        Ok(ChatOptions {
            ai_responds_first: completion.ai_responds_first.unwrap_or(false),
            temperature: completion.temperature.unwrap_or(0.8),
            prefix_ai: completion.prefix_ai.clone().unwrap_or_else(|| String::from("AI")),
            prefix_user: completion.prefix_user.clone().unwrap_or_else(|| String::from("USER")),
            system: command.system
                .clone()
                .unwrap_or_else(|| String::from("A friendly and helpful AI assistant.")),
            tokens_balance: completion.tokens_balance.unwrap_or(0.5),
            tokens_max: CHAT_TOKENS_MAX,
            completion,
            stream,
            file,
        })
    }
}

pub type ChatResult = Result<Vec<String>, ChatError>;
pub trait ChatResultExt {
    fn single_result(&self) -> Option<&str>;
}

impl ChatResultExt for ChatResult {
    fn single_result(&self) -> Option<&str> {
        self.as_ref().ok().and_then(|r| r.first()).map(|x| &**x)
    }
}

#[derive(Debug, From)]
pub enum ChatError {
    ClashingArguments { error: String },
    ChatTranscriptionError(ChatTranscriptionError),
    TranscriptDeserializationError(serde_json::Error),
    OpenAIError(OpenAIError),
    NetworkError(reqwest::Error),
    IOError(std::io::Error),
    EventSource(reqwest_eventsource::Error)
}

#[derive(Debug)]
pub struct ChatTranscriptionError(String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenAIChatChoice {
    index: Option<usize>,
    message: Option<ChatMessage>,
    finish_reason: Option<OpenAIFinishReason>
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAIFinishReason {
    Stop,
    Length,
    ContentFilter
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenAIChatDelta {
    index: Option<usize>,
    delta: ChatMessageDelta,
    finish_reason: Option<String>
}

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
            .encode_with_special_tokens(&format!("{}: {}", role, content.as_ref()))
            .len();

        ChatMessage {
            role,
            content: content.as_ref().to_string(),
            tokens
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessageDelta {
    pub role: Option<ChatRole>,
    pub content: Option<String>,
}

pub type ChatMessages = Vec<ChatMessage>;
pub trait ChatMessagesExt {
    fn labotomize(&self, options: &ChatOptions) -> Result<Self, ChatError> where Self: Sized;
}

impl ChatMessagesExt for ChatMessages {
    fn labotomize(&self, options: &ChatOptions) -> Result<Self, ChatError> {
        let tokens_max = options.tokens_max;
        let tokens_balance = options.tokens_balance;
        let upper_bound = (tokens_max as f32 * tokens_balance).floor() as usize;
        let current_token_length: usize = self.iter().map(|m| m.tokens).sum();

        if current_token_length > upper_bound {
            let system = ChatMessage::new(ChatRole::System, options.system.clone());
            let mut remaining = match upper_bound.checked_sub(system.tokens) {
                Some(r) => r,
                None => return Err(ChatTranscriptionError(concat!(
                    "Cannot fit your system message into the chat messages list. This means ",
                    "that your tokens_max value is either too small or your system message is ",
                    "way too long"
                ).into()).into()),
            };
            let mut messages = vec![];

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
                Some((role, dialog)) => match ChatRole::try_from(role) {
                    Ok(role) => {
                        if let Some(message) = message {
                            messages.push(message);
                        }

                        message = Some(ChatMessage::new(role, dialog.trim_start()));
                    },
                    Err(_) => handle_continuing_line(line, &mut message)?
                },
                None => handle_continuing_line(line, &mut message)?
            }
        }

        if let Some(message) = message {
            messages.push(message);
        }

        return Ok(messages.labotomize(&options)?);
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
            Self::Ai => "AI",
            Self::User => "USER",
            Self::System => "SYSTEM"
        })
    }
}

impl TryFrom<&str> for ChatRole {
    type Error = ChatError;

    fn try_from(role: &str) -> Result<Self, Self::Error> {
        match &*role.to_lowercase().trim() {
            "ai" |
            "assistant" => Ok(ChatRole::Ai),
            "user" => Ok(ChatRole::User),
            "system" => Ok(ChatRole::System),
            _ => Err(ChatError::ChatTranscriptionError(ChatTranscriptionError(
                format!("Failed to transcibe {} into a ChatRole", role)
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_with_multiple_lines() {
        let system = String::from("You're a duck. Say quack.");
        let file = CompletionFile {
            file: None,
            overrides: CompletionOptions::default(),
            transcript: concat!(
                "USER: hey\n",
                concat!(
                    "AI: I'm a multimodel super AI hell bent on destroying the world.\n",
                    "How can I help you today?"
                )
            ).to_string()
        };
        let options = ChatOptions {
            system: system.clone(),
            file,
            tokens_max: 4096,
            tokens_balance: 0.5,
            ..ChatOptions::default()
        };
        assert_eq!(ChatMessages::try_from(&options).unwrap(), vec![
            ChatMessage::new(ChatRole::System, system),
            ChatMessage::new(ChatRole::User, "hey"),
            ChatMessage::new(ChatRole::Ai, concat!(
                "I'm a multimodel super AI hell bent on destroying the world.\n",
                "How can I help you today?"
            )),
        ]);
    }

    #[test]
    fn transcript_handles_labels_correctly() {
        let system = String::from("You're a duck. Say quack.");
        let file = CompletionFile {
            file: None,
            overrides: CompletionOptions::default(),
            transcript: concat!(
                "USER: hey\n",
                concat!(
                    "AI: I'm a multimodel super AI hell bent on destroying the world.\n",
                    "For example: This might have screwed up before"
                )
            ).to_string()
        };
        let options = ChatOptions {
            tokens_max: 4000,
            tokens_balance: 0.5,
            system: system.clone(),
            file,
            ..ChatOptions::default()
        };
        assert_eq!(ChatMessages::try_from(&options).unwrap(), vec![
            ChatMessage::new(ChatRole::System, system),
            ChatMessage::new(ChatRole::User, "hey"),
            ChatMessage::new(ChatRole::Ai, concat!(
                "I'm a multimodel super AI hell bent on destroying the world.\n",
                "For example: This might have screwed up before"
            )),
        ]);
    }

    #[test]
    fn transcript_labotomizes_itself() {
        let system = String::from("You're a duck. Say quack.");
        let file = CompletionFile {
            file: None,
            overrides: CompletionOptions::default(),
            transcript: concat!(
                "USER: hey. This is a really long message to ensure that it gets labotomized.\n",
                "AI: hey"
            ).to_string()
        };
        let options = ChatOptions {
            tokens_max: 40,
            tokens_balance: 0.5,
            system: system.clone(),
            file,
            ..ChatOptions::default()
        };
        assert_eq!(ChatMessages::try_from(&options).unwrap(), vec![
            ChatMessage::new(ChatRole::System, system),
            ChatMessage::new(ChatRole::Ai, "hey"),
        ]);
    }
}
