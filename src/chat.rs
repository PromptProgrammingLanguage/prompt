use async_recursion::async_recursion;
use clap::{Args};
use serde::{Serialize,Deserialize};
use reqwest::Client;
use derive_more::From;
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
    pub system: Option<String>
}

impl ChatCommand {
    #[async_recursion]
    pub async fn run(&self, client: &Client, config: &Config) -> ChatResult {
        let mut options = ChatOptions::try_from((self, config))?;
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

        let mut command = OpenAIChatCommand::try_from(options)?;
        command.run(client).await
    }
}

#[derive(Default)]
pub(crate) struct ChatOptions {
    pub ai_responds_first: bool,
    pub completion: CompletionOptions,
    pub system: String,
    pub file: CompletionFile<ChatCommand>,
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
            temperature: completion.temperature.unwrap_or(0.8),
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
    ClashingArguments(ClashingArgumentsError),
    ChatTranscriptionError(ChatTranscriptionError),
    TranscriptDeserializationError(serde_json::Error),
    OpenAIError(OpenAIError),
    NetworkError(reqwest::Error),
    IOError(std::io::Error),
    EventSource(reqwest_eventsource::Error)
}

#[derive(Debug)]
pub struct ChatTranscriptionError(pub String);
