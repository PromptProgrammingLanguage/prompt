use async_recursion::async_recursion;
use clap::{Args,ValueEnum};
use reqwest::Client;
use derive_more::From;
use serde::{Serialize,Deserialize};
use crate::openai::{OpenAISessionCommand,OpenAIError};
use crate::cohere::session::{CohereSessionCommand,CohereError};
use crate::completion::{CompletionFile,CompletionOptions,ClashingArgumentsError};
use crate::Config;

#[derive(Args, Clone, Default, Debug, Serialize, Deserialize)]
pub struct SessionCommand {
    #[command(flatten)]
    #[serde(flatten)]
    pub completion: CompletionOptions,

    /// Model size
    #[arg(value_enum, long, short)]
    pub model: Option<Model>,

    /// Model focus
    #[arg(value_enum, long)]
    pub model_focus: Option<ModelFocus>,

    /// Prompt
    #[arg(short, long)]
    pub prompt: Option<String>,

    /// Prompt path
    #[arg(long)]
    pub prompt_path: Option<String>,

    /// Provider
    #[arg(long)]
    pub provider: Option<Provider>,
}

#[derive(Debug, Default)]
pub(crate) struct SessionOptions {
    pub ai_responds_first: bool,
    pub completion: CompletionOptions,
    pub file: CompletionFile<SessionCommand>,
    pub model: Model,
    pub model_focus: ModelFocus,
    pub prompt: String,
    pub stream: bool,
    pub no_context: bool,
    pub provider: Provider,
}

impl TryFrom<(&SessionCommand, &Config)> for SessionOptions {
    type Error = SessionError;

    fn try_from((command, config): (&SessionCommand, &Config)) -> Result<Self, Self::Error> {
        let file = command.completion.load_session_file::<SessionCommand>(config, command.clone());
        let completion = if file.file.is_some() {
            command.completion.merge(&file.overrides.completion)
        } else {
            command.completion.clone()
        };

        completion.validate()?;

        Ok(SessionOptions {
            ai_responds_first: completion.ai_responds_first.unwrap_or(false),
            stream: completion.parse_stream_option()?,
            prompt: command.parse_prompt_option(),
            no_context: command.parse_no_context_option(),
            model: command.model.unwrap_or(Model::XXLarge),
            model_focus: command.model_focus.unwrap_or(ModelFocus::Text),
            provider: command.provider.unwrap_or(Provider::OpenAI),
            completion,
            file
        })
    }
}

pub type SessionResult = Result<Vec<String>, SessionError>;
pub trait SessionResultExt {
    fn single_result(&self) -> Option<&str>;
}

impl SessionResultExt for SessionResult {
    fn single_result(&self) -> Option<&str> {
        self.as_ref().ok().and_then(|r| r.first()).map(|x| &**x)
    }
}

#[derive(From, Debug)]
pub enum SessionError {
    NoMatchingModel,
    TemperatureOutOfValidRange,
    ClashingArguments(ClashingArgumentsError),
    CohereError(CohereError),
    OpenAIError(OpenAIError),
    IOError(std::io::Error),
    DeserializeError(reqwest::Error),
    Unauthorized
}

impl SessionCommand {
    #[async_recursion]
    pub async fn run(&self, client: &Client, config: &Config) -> SessionResult {
        let mut options = SessionOptions::try_from((self, config))?;
        let prefix_user = options.completion.prefix_user.as_ref().map(|u| &**u);

        // The commands need to be instantiated before printing the opening prompt because they can
        // print warnings about mismatched options without failing.
        let command = match options.provider {
            Provider::OpenAI => Ok(OpenAISessionCommand::try_from(&options)?),
            Provider::Cohere => Err(CohereSessionCommand::try_from(&options)?),
        };

        let print_output = !options.completion.quiet.unwrap_or(false);
        if print_output && options.file.transcript.len() > 0 {
            println!("{}", options.file.transcript);
        }

        let line = if options.ai_responds_first {
            String::new()
        } else {
            let append = options.completion.append.as_ref().map(|a| &**a);

            if let Some(line) = options.file.read(append, prefix_user, options.no_context) {
                line
            } else {
                return Ok(vec![]);
            }
        };

        loop {
            let transcript = &options.file.transcript;
            let prompt = &options.prompt;
            let prompt = match (options.no_context, &options.completion.prefix_ai) {
                (true, None) => prompt.replace("${TRANSCRIPT}", &line),
                (true, Some(prefix)) => prompt.replace("${TRANSCRIPT}", &line) + &prefix,
                (false, None) => prompt.replace("${TRANSCRIPT}", transcript),
                (false, Some(prefix)) =>
                    prompt.replace("${TRANSCRIPT}", transcript) + &prefix
            };

            let result = match &command {
                Ok(command) => command.run(client, config, &prompt).await?,
                Err(command) => command.run(client, config, &prompt).await?,
            };

            if let Some(count) = options.completion.response_count {
                if count > 1 {
                    return Ok(result);
                }
            }

            let text = result.first().unwrap().trim();
            let written_response = match &options.completion.prefix_ai {
                Some(prefix) => format!("{}{}", prefix, text),
                None => text.to_owned()
            };
            let text = options.file.write(text.into(), options.no_context, false)?;

            if !options.completion.quiet.unwrap_or(false) {
                println!("{}", written_response);
            }

            if options.completion.append.is_some() {
                return Ok(vec![ text.to_string() ]);
            }

            if let None = options.file.read(None, prefix_user, options.no_context) {
                return Ok(vec![]);
            }
        }
    }

    pub fn parse_no_context_option(&self) -> bool {
        self.completion.no_context.unwrap_or_else(|| {
            match self.model_focus {
                Some(ModelFocus::Code) => true,
                _ => false,
            }
        })
    }

    pub fn parse_prompt_option(&self) -> String {
        self.prompt
            .clone()
            .or_else(|| {
                self.prompt_path
                    .clone()
                    .and_then(|path| {
                        std::fs::read_to_string(path).ok()
                    })
            })
            .unwrap_or_else(|| {
                match self.model_focus {
                    Some(ModelFocus::Text) | None => DEFAULT_CHAT_PROMPT_WRAPPER.to_owned(),
                    Some(ModelFocus::Code) => DEFAULT_CODE_PROMPT_WRAPPER.to_owned(),
                }
            })
    }
}

#[derive(Copy, Clone, Debug, Default, ValueEnum, Serialize, Deserialize)]
pub enum Provider {
    /// Cohere
    Cohere,

    /// OpenAI
    #[default]
    OpenAI,
}

#[derive(Copy, Clone, Debug, Default, ValueEnum, Serialize, Deserialize)]
pub enum Model {
    /// In the range of 0 - 1 billion parameters. OpenAI's Ada, Cohere's "small" option.
    Tiny,

    /// In the range of 1 - 5 billion parameters. OpenAI's Babbage option.
    Small,

    /// In the range of 5 - 10 billion parameters. OpenAI's Curie, Cohere's "medium" option.
    Medium,

    /// In the range of 10 - 50 billion parameters. Cohere's large option.
    Large,

    /// In the range of 50 - 150 billion paramaters. Cohere's xlarge option.
    XLarge,

    /// Greater than 150 billion paramaters. OpenAI's davinci model.
    #[default]
    XXLarge
}

#[derive(Copy, Clone, Default, Debug, ValueEnum, Serialize, Deserialize)]
pub enum ModelFocus {
    Code,
    #[default]
    Text
}


const DEFAULT_CODE_PROMPT_WRAPPER: &str = "${TRANSCRIPT}";
const DEFAULT_CHAT_PROMPT_WRAPPER: &str = "
The following is a transcript between a helpful AI assistant and a human. The AI assistant can provide factual information (but only from before mid 2021, when its training data cuts off), ask clarifying questions, and engage in chit chat.

Transcript:

${TRANSCRIPT}

";
