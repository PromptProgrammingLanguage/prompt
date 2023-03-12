use crate::chat::{ChatOptions,ChatResult,ChatError,ChatTranscriptionError};
use std::io::{self,Write};
use async_recursion::async_recursion;
use serde::{Serialize,Deserialize};
use reqwest::Client;
use reqwest_eventsource::{EventSource,Event};
use serde_json::json;
use futures_util::stream::StreamExt;
use crate::openai::response::OpenAICompletionResponse;
use tiktoken_rs::p50k_base;

pub struct OpenAIChatCommand {
    options: ChatOptions
}

impl TryFrom<ChatOptions> for OpenAIChatCommand {
    type Error = ChatError;

    fn try_from(options: ChatOptions) -> Result<Self, Self::Error> {
        Ok(OpenAIChatCommand { options })
    }
}

impl OpenAIChatCommand {
    #[async_recursion]
    pub async fn run(&mut self, client: &Client) -> ChatResult {
        let options = &mut self.options;
        let print_output = !options.completion.quiet.unwrap_or(false);

        loop {
            if options.stream {
                let result = handle_stream(client, options).await?;
                if result.len() > 0 {
                    return Ok(result);
                }
            } else {
                let result = handle_sync(client, options, print_output).await?;
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
    let mut state = StreamMessageState::New;

    'stream: while let Some(event) = stream.next().await {
        match event {
            Ok(Event::Open) => {},
            Ok(Event::Message(message)) if message.data == "[DONE]" => {
                break 'stream;
            },
            Ok(Event::Message(message)) => {
                state = handle_stream_message(options, message.data, state)?;
            },
            Err(err) => {
                stream.close();
                return Err(ChatError::EventSource(err));
            }
        }
    }

    match state {
        StreamMessageState::New => {},
        StreamMessageState::HasWrittenRole |
        StreamMessageState::HasWrittenContent => {
            println!("");
            options.file.write_words(String::from("\n"))?;
            io::stdout().flush().unwrap();
        },
    }

    Ok(vec![])
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum StreamMessageState {
    New,
    HasWrittenRole,
    HasWrittenContent,
}

fn handle_stream_message(
    options: &mut ChatOptions,
    message: String,
    mut state: StreamMessageState) -> Result<StreamMessageState, ChatError>
{
    let chat_response: OpenAICompletionResponse<OpenAIChatDelta> =
        serde_json::from_str(&message)?;

    let delta = &chat_response.choices.first().unwrap().delta;
    if let Some(ref role) = delta.role {
        let role = options.file.write_words(format!("{}", role))?;
        print!("{}", role);
        state = StreamMessageState::HasWrittenRole;
    }
    if let Some(content) = delta.content.clone() {
        let filtered = match state {
            StreamMessageState::New |
            StreamMessageState::HasWrittenRole => {
                let filtered = content.trim_start();
                let prefix_ai = &format!("{}:", options.prefix_ai);

                if filtered.starts_with(prefix_ai) {
                    filtered
                        .replacen(prefix_ai, "", 1)
                        .trim_start()
                        .to_string()
                } else {
                    filtered.to_string()
                }
            },
            StreamMessageState::HasWrittenContent => content,
        };

        print!("{}", filtered);
        state = StreamMessageState::HasWrittenContent;
        options.file.write_words(format!("{}", filtered))?;
    }
    io::stdout().flush().unwrap();
    Ok(state)
}

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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
            .encode_with_special_tokens(&format!("{}{}", role, content.as_ref()))
            .len();

        ChatMessage {
            role,
            content: content.as_ref().to_string(),
            tokens
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatMessageDelta {
    pub role: Option<ChatRole>,
    pub content: Option<String>,
}

pub type ChatMessages = Vec<ChatMessage>;
trait ChatMessagesInternalExt {
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
            Self::Ai => "AI: ",
            Self::User => "USER: ",
            Self::System => "SYSTEM: "
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
    use crate::chat::*;
    use crate::completion::*;

    #[test]
    fn transcript_with_multiple_lines() {
        let system = String::from("You're a duck. Say quack.");
        let file = CompletionFile {
            file: None,
            overrides: ChatCommand::default(),
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
            overrides: ChatCommand::default(),
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
            overrides: ChatCommand::default(),
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

    #[test]
    fn streaming_strips_whitespace_and_labels_from_delta_content() {
        let file = CompletionFile {
            file: None,
            overrides: ChatCommand::default(),
            transcript: String::new()
        };
        let mut options = ChatOptions {
            tokens_max: 40,
            tokens_balance: 0.5,
            prefix_ai: "AI".into(),
            file,
            ..ChatOptions::default()
        };
        let chat_response = String::from(r#"{
            "choices": [
                {
                    "delta": {
                        "role": "assistant",
                        "content": "\n     AI: hey there"
                    }
                }
            ],
            "created": 0,
            "model": "",
            "object": "",
            "id": ""
        }"#);

        let state = handle_stream_message(&mut options, chat_response, StreamMessageState::New)
            .unwrap();

        assert_eq!(StreamMessageState::HasWrittenContent, state);
        assert_eq!("AI: hey there", &options.file.transcript)
    }
}
