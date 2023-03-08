use serde_json::json;
use serde::Deserialize;
use crate::session::{SessionResult,SessionError,ModelFocus,Model};
use crate::{Config,SessionCommand};
use reqwest::Client;
use super::response::OpenAICompletionResponse;

#[derive(Debug, Default)]
pub struct OpenAISessionCommand {
    pub command: SessionCommand,
    pub temperature: OpenAITemperature,
    pub model: OpenAIModel
}

impl TryFrom<&SessionCommand> for OpenAISessionCommand {
    type Error = SessionError;

    fn try_from(command: &SessionCommand) -> Result<Self, SessionError> {
        Ok(Self {
            command: command.clone(),
            temperature: OpenAITemperature::try_from(command.temperature)?,
            model: OpenAIModel::try_from((command.model_focus, command.model))?
        })
    }
}

impl OpenAISessionCommand {
    pub async fn run(&self,
        client: &Client,
        config: &Config,
        prompt: &str) -> SessionResult
    {
        let mut post = client.post("https://api.openai.com/v1/completions");

        if let Some(key) = &config.api_key_openai {
            post = post.bearer_auth(key);
        }

        let request = post
            .json(&json!({
                "model": self.model.to_versioned(),
                "prompt": &prompt,
                "max_tokens": 1000,
                "temperature": self.temperature.0,
                "n": self.command.response_count.unwrap_or(1)
            }))
            .send()
            .await
            .expect("Failed to send completion");

        if !request.status().is_success() {
            return Err(SessionError::OpenAIError(request.json().await?));
        }

        let session_response: OpenAICompletionResponse<OpenAISessionChoice> = request.json().await?;
        Ok(session_response.choices.into_iter().map(|r| r.text).collect())
    }
}

#[derive(Clone, Debug, Default)]
pub struct OpenAITemperature(pub f32);

impl TryFrom<f32> for OpenAITemperature {
    type Error = SessionError;

    fn try_from(n: f32) -> Result<Self, SessionError> {
        match n.floor() as u32 {
            0..=2 => Ok(OpenAITemperature(n)),
            _ => Err(SessionError::TemperatureOutOfValidRange)
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpenAIModel {
    #[default]
    TextDavinci,
    TextCurie,
    TextBabbage,
    TextAda,
    CodeDavinci,
    CodeCushman
}

impl OpenAIModel {
    pub fn to_versioned(&self) -> &str {
        match self {
            OpenAIModel::TextDavinci => "text-davinci-003",
            OpenAIModel::TextCurie => "text-curie-001",
            OpenAIModel::TextBabbage => "text-babbage-001",
            OpenAIModel::TextAda => "text-ada-001",
            OpenAIModel::CodeDavinci => "code-davinci-002",
            OpenAIModel::CodeCushman => "code-cushman-001",
        }
    }
}

macro_rules! warn_inexact_match{
    ($size:expr,$focus:expr)=>{
        {
            eprintln!(concat!(
                "warning: No exact matching OpenAI model for ",$size," option with a ",$focus," ",
                "focus. Falling back to ",$size," option with a ",$focus," focus."));
        }
    }
}

impl TryFrom<(ModelFocus, Model)> for OpenAIModel {
    type Error = SessionError;

    fn try_from(models: (ModelFocus, Model)) -> Result<OpenAIModel, SessionError> {
        Ok(match models {
            (ModelFocus::Text, Model::Tiny) => OpenAIModel::TextAda,
            (ModelFocus::Code, Model::Tiny) |
            (ModelFocus::Code, Model::Small) => {
                return Err(SessionError::NoMatchingModel)
            },
            (ModelFocus::Text, Model::Small) => OpenAIModel::TextBabbage,
            (ModelFocus::Text, Model::Medium) => OpenAIModel::TextCurie,
            (ModelFocus::Code, Model::Medium) => OpenAIModel::CodeCushman,
            (ModelFocus::Text, Model::Large) => {
                warn_inexact_match!("large", "text");
                OpenAIModel::TextCurie
            },
            (ModelFocus::Code, Model::Large) => {
                warn_inexact_match!("large", "code");
                OpenAIModel::CodeCushman
            },
            (ModelFocus::Text, Model::XLarge) => {
                warn_inexact_match!("x-large", "text");
                OpenAIModel::TextCurie
            },
            (ModelFocus::Code, Model::XLarge) => {
                warn_inexact_match!("x-large", "code");
                OpenAIModel::CodeCushman
            },
            (ModelFocus::Code, Model::XXLarge) => OpenAIModel::CodeDavinci,
            (ModelFocus::Text, Model::XXLarge) => OpenAIModel::TextDavinci,
        })
    }
}

#[derive(Deserialize)]
pub struct OpenAISessionChoice {
    pub text: String,
    pub index: u32,
    pub logprobs: Option<u32>,
    pub finish_reason: Option<String>
}
