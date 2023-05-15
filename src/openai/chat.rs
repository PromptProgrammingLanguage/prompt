use crate::chat::{ChatOptions,ChatResult,ChatMessage,ChatProvider,ChatMessages,ChatRole,ChatError};
use crate::openai::response::OpenAICompletionResponse;
use crate::completion::ClashingArgumentsError;
use crate::Config;
use std::io::{self,Write};
use std::env;
use std::cmp;
use async_recursion::async_recursion;
use serde::{Serialize,Deserialize};
use reqwest::{Client,RequestBuilder};
use reqwest_eventsource::{EventSource,Event};
use serde_json::{json,Map};
use futures_util::stream::StreamExt;
use tiktoken_rs::get_chat_completion_max_tokens;
use async_openai::types::{ChatCompletionRequestMessageArgs, Role};

const MAX_GPT3_TURBO_TOKENS: usize = 4096;
const MAX_GPT4_BASE_TOKENS: usize = 8192;
const MAX_GPT4_EXTENDED_TOKENS: usize = 32768;

#[derive(Debug)]
pub struct OpenAIChatCommand {
    options: ChatOptions,
}

impl TryFrom<ChatOptions> for OpenAIChatCommand {
    type Error = ChatError;

    fn try_from(mut options: ChatOptions) -> Result<Self, Self::Error> {
        let provider = options.provider;
        let tokens_max = get_max_tokens_for_model(provider);
        let is_exceeding_max_tokens_allowed = match provider {
            ChatProvider::OpenAiGPT3Turbo |
            ChatProvider::OpenAiGPT3Turbo_0301 if tokens_max > MAX_GPT3_TURBO_TOKENS => true,

            ChatProvider::OpenAiGPT4 |
            ChatProvider::OpenAiGPT4_0314 if tokens_max > MAX_GPT4_BASE_TOKENS => true,

            ChatProvider::OpenAiGPT4_32K |
            ChatProvider::OpenAiGPT4_32K_0314 if tokens_max > MAX_GPT4_EXTENDED_TOKENS => true,

            _ => false
        };

        options.tokens_max = Some(tokens_max);

        if is_exceeding_max_tokens_allowed {
            return Err(ClashingArgumentsError::new(format!(
                r#"Max tokens "{tokens_max}" exceeds max allowed length for "{provider}""#)))?
        }

        if options.stop.len() > 4 {
            return Err(ClashingArgumentsError::new(format!(
                r#"Cannot surpass more then 4 stops for "{provider}""#)))?
        }

        Ok(OpenAIChatCommand {
            options,
        })
    }
}

impl OpenAIChatCommand {
    #[async_recursion]
    pub async fn run(&mut self, client: &Client, config: &Config) -> ChatResult {
        let options = &mut self.options;
        let print_output = !options.completion.quiet.unwrap_or(false);

        loop {
            if options.stream {
                let result = handle_stream(client, options, config).await?;
                if result.len() > 0 {
                    return Ok(result);
                }
            } else {
                let result = handle_sync(client, options, config, print_output).await?;
                if result.len() > 0 {
                    return Ok(result);
                }
            }

            if let None = options.file.read(None, Some(&*options.prefix_user), options.no_context) {
                return Ok(vec![]);
            }
        }
    }
}

async fn handle_sync(client: &Client, options: &mut ChatOptions, config: &Config, print_output: bool) -> ChatResult {
    let request = get_request(&client, &options, &config, false)?
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

            if message.to_lowercase().starts_with(&options.prefix_ai) {
                message.to_string()
            } else {
                format!("{}: {}", options.prefix_ai, message)
            }
        });

    if let Some(text) = text {
        let text = options.file.write(text, options.no_context, false)?;

        if print_output {
            println!("{}", text);
        }

        if options.completion.append.is_some() || options.completion.once.unwrap_or(false) {
            return Ok(ChatMessages::try_from(&*options)?);
        }
    }

    Ok(vec![])
}

async fn handle_stream(client: &Client, options: &mut ChatOptions, config: &Config) -> ChatResult {
    let post = get_request(client, options, config, true)?;
    let mut stream = EventSource::new(post).unwrap();
    let mut state = StreamMessageState::New;
    let mut response = String::new();

    'stream: while let Some(event) = stream.next().await {
        match event {
            Ok(Event::Open) => {},
            Ok(Event::Message(message)) if message.data == "[DONE]" => {
                break 'stream;
            },
            Ok(Event::Message(message)) => {
                state = handle_stream_message(options, message.data, &mut response, state)?;
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
            response += "\n";
            io::stdout().flush().unwrap();
        },
    }

    options.file.write(response, options.no_context, false)?;

    if options.completion.append.is_some() || options.completion.once.unwrap_or(false) {
        return Ok(ChatMessages::try_from(&*options)?);
    }

    Ok(vec![])
}

const DEFAULT_OPEN_API_URL: &'static str = "https://api.openai.com";

fn get_request(client: &Client, options: &ChatOptions, config: &Config, stream: bool) -> Result<RequestBuilder, ChatError> {
    let base_url = env::var("OPEN_AI_PROXY_URL").unwrap_or_else(|_| DEFAULT_OPEN_API_URL.into());
    let model = format!("{}", options.provider);
    let messages = ChatMessages::try_from(options)?;
    let max_tokens = options.tokens_max
        .unwrap_or_else(|| get_max_tokens_for_model(options.provider));

    let mut map = Map::new();
    map.insert("temperature".to_string(), options.temperature.into());
    map.insert("stream".to_string(), stream.into());
    map.insert("max_tokens".to_string(), cmp::min(max_tokens, get_max_allowed_tokens(&model, &messages)).into());
    map.insert("model".to_string(), model.into());
    map.insert("messages".to_string(), serde_json::to_value(messages)?);

    if options.stop.len() > 0 {
        map.insert("stop".to_string(), options.stop.clone().into());
    }

    Ok(client.post(&format!("{base_url}/v1/chat/completions"))
        .bearer_auth(env::var("OPEN_AI_API_KEY")
            .ok()
            .or_else(|| config.api_key_openai.clone())
            .ok_or_else(|| ChatError::Unauthorized)?
        )
        .json(&serde_json::Value::Object(map))
    )
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
    response: &mut String,
    mut state: StreamMessageState) -> Result<StreamMessageState, ChatError>
{
    let chat_response: OpenAICompletionResponse<OpenAIChatDelta> =
        serde_json::from_str(&message)?;

    let delta = &chat_response.choices.first().unwrap().delta;
    if let Some(ref role) = delta.role {
        print!("{}", role);
        response.push_str(&format!("{role}"));
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
        response.push_str(&filtered);
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatMessageDelta {
    pub role: Option<ChatRole>,
    pub content: Option<String>,
}

fn get_max_tokens_for_model(provider: ChatProvider) -> usize {
    match provider {
        ChatProvider::OpenAiGPT3Turbo |
        ChatProvider::OpenAiGPT3Turbo_0301 => MAX_GPT3_TURBO_TOKENS,

        ChatProvider::OpenAiGPT4 |
        ChatProvider::OpenAiGPT4_0314 => MAX_GPT4_BASE_TOKENS,

        ChatProvider::OpenAiGPT4_32K |
        ChatProvider::OpenAiGPT4_32K_0314 => MAX_GPT4_EXTENDED_TOKENS,
    }
}

fn get_max_allowed_tokens(model: &str, messages: &ChatMessages) -> usize {
    let messages = messages.clone().into_iter()
        .map(|m| ChatCompletionRequestMessageArgs::default()
            .content(m.content)
            .role(match m.role {
                ChatRole::User => Role::User,
                ChatRole::Ai => Role::Assistant,
                ChatRole::System => Role::System
            })
            .build()
            .unwrap()
        )
        .collect::<Vec<_>>();

    get_chat_completion_max_tokens(&model, &messages).unwrap() - 1
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
            transcript: concat!(
                "USER: hey\n",
                concat!(
                    "AI: I'm a multimodel super AI hell bent on destroying the world.\n",
                    "How can I help you today?"
                )
            ).to_string(),
            ..CompletionFile::default()
        };
        let options = ChatOptions {
            system: system.clone(),
            file,
            tokens_max: Some(4096),
            tokens_balance: 0.5,
            ..ChatOptions::default()
        };
        assert_eq!(ChatMessages::try_from(&options).unwrap(), vec![
            ChatMessage::new(ChatRole::System, system),
            ChatMessage::new(ChatRole::User, "USER: hey"),
            ChatMessage::new(ChatRole::Ai, concat!(
                "AI: I'm a multimodel super AI hell bent on destroying the world.\n",
                "How can I help you today?"
            )),
        ]);
    }

    #[test]
    fn transcript_handles_labels_correctly() {
        let system = String::from("You're a duck. Say quack.");
        let file = CompletionFile {
            transcript: concat!(
                "USER: hey\n",
                concat!(
                    "AI: I'm a multimodel super AI hell bent on destroying the world.\n",
                    "For example: This might have screwed up before"
                )
            ).to_string(),
            ..CompletionFile::default()
        };
        let options = ChatOptions {
            tokens_max: Some(4000),
            tokens_balance: 0.5,
            system: system.clone(),
            file,
            ..ChatOptions::default()
        };
        assert_eq!(ChatMessages::try_from(&options).unwrap(), vec![
            ChatMessage::new(ChatRole::System, system),
            ChatMessage::new(ChatRole::User, "USER: hey"),
            ChatMessage::new(ChatRole::Ai, concat!(
                "AI: I'm a multimodel super AI hell bent on destroying the world.\n",
                "For example: This might have screwed up before"
            )),
        ]);
    }

    #[test]
    fn transcript_labotomizes_itself() {
        let system = String::from("You're a duck. Say quack.");
        let file = CompletionFile {
            transcript: concat!(
                "USER: hey. This is a really long message to ensure that it gets labotomized.\n",
                "AI: hey"
            ).to_string(),
            ..CompletionFile::default()
        };
        let options = ChatOptions {
            tokens_max: Some(40),
            tokens_balance: 0.5,
            system: system.clone(),
            file,
            ..ChatOptions::default()
        };
        assert_eq!(ChatMessages::try_from(&options).unwrap(), vec![
            ChatMessage::new(ChatRole::System, system),
            ChatMessage::new(ChatRole::Ai, "AI: hey"),
        ]);
    }

    #[test]
    fn streaming_strips_whitespace_and_labels_from_delta_content() {
        let file = CompletionFile::default();
        let mut options = ChatOptions {
            tokens_max: Some(40),
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

        let mut response = String::new();
        let state = handle_stream_message(
                &mut options,
                chat_response,
                &mut response,
                StreamMessageState::New)
            .unwrap();

        assert_eq!(StreamMessageState::HasWrittenContent, state);
        assert_eq!("AI: hey there", &options.file.transcript)
    }
}
