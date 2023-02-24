use serde_json::json;
use serde::{Deserialize,Serialize};
use crate::session::{SessionResult,SessionError,Model,ModelFocus};
use crate::{Config,SessionCommand};
use reqwest::Client;
use reqwest::header::HeaderValue;
use uuid::Uuid;

#[derive(Default)]
pub struct CohereSessionCommand {
    pub command: SessionCommand,
    pub model: CohereModel,
    pub temperature: CohereTemperature,
}

impl TryFrom<&SessionCommand> for CohereSessionCommand {
    type Error = SessionError;

    fn try_from(command: &SessionCommand) -> Result<Self, SessionError> {
        match command.model_focus {
            ModelFocus::Code => { return Err(SessionError::NoMatchingModel); },
            _ => {}
        }

        Ok(Self {
            command: command.clone(),
            temperature: CohereTemperature::try_from(command.temperature)?,
            model: CohereModel::try_from(command.model)?
        })
    }
}

impl CohereSessionCommand {
    pub async fn run(&self,
        client: &Client,
        config: &Config,
        prompt: &str) -> SessionResult
    {
        let mut post = client.post("https://api.cohere.ai/generate");
        if let Some(key) = &config.api_key_cohere {
            post = post.bearer_auth(key);
        }
        let request = post
            .header("Cohere-Version", HeaderValue::from_static("2022-12-06"))
            .json(&json!({
                "model": self.model.to_versioned(),
                "prompt": &prompt,
                "max_tokens": 100,
                "return_likelihoods": "NONE",
                "truncate": "NONE",
                "num_generations": self.command.response_count.unwrap_or(1),
                "temperature": 5,
                "stop_sequences": [ "HUMAN:", "AI:" ]
            }))
            .send()
            .await
            .expect("Failed to send completion");

        if !request.status().is_success() {
            let error: CohereError = request.json()
                .await
                .expect("Unkown json response from Cohere");

            return Err(SessionError::CohereError(error));
        }

        let response: CohereSessionResponse = request.json()
            .await
            .expect("Unkown json response from Cohere");

        Ok(response.generations.into_iter().map(|c| c.text).collect())
    }
}

#[derive(Default)]
pub enum CohereModel {
    Small,
    Medium,
    Large,
    #[default]
    XLarge
}

impl CohereModel {
    fn to_versioned(&self) -> &str {
        match self {
            CohereModel::Small => "small",
            CohereModel::Medium => "medium",
            CohereModel::Large => "large",
            CohereModel::XLarge => "xlarge"
        }
    }
}

impl TryFrom<Model> for CohereModel {
    type Error = SessionError;

    fn try_from(model: Model) -> Result<Self, SessionError> {
        Ok(match model {
            Model::Tiny => CohereModel::Small,
            Model::Small => {
                eprintln!(concat!(
                    "warning: Cohere doesn't actually have a small model by AI's definition. ",
                    "Falling back to the tiny model."));
                CohereModel::Small
            },
            Model::Medium => CohereModel::Medium,
            Model::Large => CohereModel::Large,
            Model::XLarge => CohereModel::XLarge,
            Model::XXLarge => {
                eprintln!(concat!(
                    "warning: Cohere doesn't have an XXLarge model by AI's definition, falling ",
                    "back to the XLarge model."));
                CohereModel::XLarge
            }
        })
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct CohereSessionResponse {
    pub id: Uuid,
    pub generations: Vec<CohereChoice>,
    pub prompt: String
}

#[derive(Clone, Deserialize, Debug)]
pub struct CohereChoice {
    pub id: Uuid,
    pub text: String,
}

#[derive(Clone, Deserialize, Debug, Serialize)]
pub struct CohereError {
    pub message: String
}

#[derive(Clone, Default)]
pub struct CohereTemperature(pub f32);

impl TryFrom<f32> for CohereTemperature {
    type Error = SessionError;

    fn try_from(n: f32) -> Result<Self, SessionError> {
        match n.floor() as u32 {
            0..=5 => Ok(CohereTemperature(n)),
            _ => Err(SessionError::TemperatureOutOfValidRange)
        }
    }
}
