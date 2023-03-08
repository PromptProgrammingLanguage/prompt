use async_recursion::async_recursion;
use clap::{Args};
use serde::{Serialize,Deserialize};
use reqwest::Client;
use derive_more::From;
use serde_json::json;
use crate::openai::response::OpenAICompletionResponse;
use crate::openai::OpenAIError;
use crate::completion::{CompletionOptions,CompletionFile};
use crate::Config;

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
        let mut options = ChatOptions::from((&*self, config));
        let prefix_user = Some(&*options.prefix_user);
        let print_output = !options.completion.quiet.unwrap_or(false);

        if print_output && options.file.transcript.len() > 0 {
            println!("{}", options.file.transcript);
        }

        if !options.ai_responds_first {
            let append = options.completion.append.as_ref().map(|a| &**a);

            if let None = options.file.read(append, prefix_user) {
                return Ok(vec![]);
            }
        }

        loop {
            let post = client.post("https://api.openai.com/v1/chat/completions");
            let request = post
                .json(&json!({
                    "model": "gpt-3.5-turbo",
                    "temperature": options.temperature,
                    "messages": transcript_to_chat_messages(&options.file, &options.system)?
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

            if let None = options.file.read(None, prefix_user) {
                return Ok(vec![]);
            }
        }
    }
}

struct ChatOptions {
    ai_responds_first: bool,
    completion: CompletionOptions,
    system: String,
    file: CompletionFile,
    prefix_ai: String,
    prefix_user: String,
    temperature: f32,
}

impl From<(&ChatCommand, &Config)> for ChatOptions {
    fn from((command, config): (&ChatCommand, &Config)) -> Self {
        let file = command.completion.load_session_file::<CompletionOptions>(config);
        let completion = if file.file.is_some() {
            command.completion.merge(&file.overrides)
        } else {
            command.completion.clone()
        };

        ChatOptions {
            ai_responds_first: completion.ai_responds_first.unwrap_or(false),
            temperature: completion.temperature.unwrap_or(0.8),
            prefix_ai: completion.prefix_ai.clone().unwrap_or_else(|| String::from("AI")),
            prefix_user: completion.prefix_user.clone().unwrap_or_else(|| String::from("USER")),
            system: command.system
                .clone()
                .unwrap_or_else(|| String::from("A friendly and helpful AI assistant.")),
            completion,
            file,
        }
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
    ChatTranscriptionError { message: String },
    TranscriptDeserializationError(serde_json::Error),
    OpenAIError(OpenAIError),
    NetworkError(reqwest::Error),
    IOError(std::io::Error)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenAIChatChoice {
    index: Option<usize>,
    message: Option<ChatMessage>,
    finish_reason: Option<String>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl AsRef<str>) -> Self {
        ChatMessage {
            role,
            content: content.as_ref().to_string()
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ChatRole {
    #[serde(rename = "assistant")]
    Ai,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "system")]
    System
}

impl TryFrom<&str> for ChatRole {
    type Error = ChatError;

    fn try_from(role: &str) -> Result<Self, Self::Error> {
        match &*role.to_lowercase().trim() {
            "ai" |
            "assistant" => Ok(ChatRole::Ai),
            "user" => Ok(ChatRole::User),
            "system" => Ok(ChatRole::System),
            _ => Err(ChatError::ChatTranscriptionError {
                message: format!("Failed to transcibe {} into a ChatRole", role)
            })
        }
    }
}

fn transcript_to_chat_messages(
    file: &CompletionFile,
    system: &str) -> Result<Vec<ChatMessage>, ChatError>
{
    file.transcript.lines()
        .map(|line| line.split_once(':')
            .ok_or(ChatError::ChatTranscriptionError { message: "Missing chat role".into() })
            .and_then(|(role, line)| ChatRole::try_from(role).map(|role| (role, line)))
            .map(|(role, line)| ChatMessage::new(role, line))
        )
        .collect::<Result<Vec<ChatMessage>, ChatError>>()
        .map(|mut messages| {
            messages.insert(0, ChatMessage::new(ChatRole::System, system));
            messages
        })
}
