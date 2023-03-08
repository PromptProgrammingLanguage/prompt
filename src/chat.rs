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
    let messages = transcript_to_chat_messages(&options.file, &options.system)?;
    let request = post
        .json(&json!({
            "model": "gpt-3.5-turbo",
            "temperature": options.temperature,
            "messages": messages
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
            "messages": transcript_to_chat_messages(&options.file, &options.system)?
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
                    let role = options.file.write_words(match role {
                        ChatRole::Ai => "AI: ".into(),
                        ChatRole::System => "SYSTEM: ".into(),
                        ChatRole::User => "USER: ".into(),
                    })?;
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

struct ChatOptions {
    ai_responds_first: bool,
    completion: CompletionOptions,
    system: String,
    file: CompletionFile,
    prefix_ai: String,
    prefix_user: String,
    stream: bool,
    temperature: f32,
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
    finish_reason: Option<String>
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessageDelta {
    pub role: Option<ChatRole>,
    pub content: Option<String>,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl AsRef<str>) -> Self {
        ChatMessage {
            role,
            content: content.as_ref().to_string()
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

fn transcript_to_chat_messages(
    file: &CompletionFile,
    system: &str) -> Result<Vec<ChatMessage>, ChatError>
{
    let mut messages = vec![];
    let mut message = None;

    messages.push(ChatMessage::new(ChatRole::System, system));

    for line in file.transcript.lines() {
        match line.split_once(':') {
            Some((role, line)) => {
                if let Some(message) = message {
                    messages.push(message);
                }

                message = Some(ChatMessage {
                    role: ChatRole::try_from(role)?,
                    content: line.trim_start().to_string()
                });
            },
            None => match message {
                Some(ref mut message) => {
                    message.content += "\n";
                    message.content += line;
                },
                None => {
                    return Err(ChatError::ChatTranscriptionError(ChatTranscriptionError(
                        "Missing opening chat role".into()
                    )));
                }
            }
        }
    }

    if let Some(message) = message {
        messages.push(message);
    }

    return Ok(messages);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_with_multiple_lines() {
        let system = "You're a duck. Say quack.";
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
        assert_eq!(transcript_to_chat_messages(&file, system).unwrap(), vec![
            ChatMessage { role: ChatRole::System, content: system.into() },
            ChatMessage { role: ChatRole::User, content: "hey".into() },
            ChatMessage {
                role: ChatRole::Ai,
                content: concat!(
                    "I'm a multimodel super AI hell bent on destroying the world.\n",
                    "How can I help you today?"
                ).into()
            },
        ]);
    }
}
