use std::io::Write;
use std::fs::File;
use serde::{Serialize,Deserialize};
use serde_json::json;
use reqwest::{Client,StatusCode};
use reqwest::header::HeaderValue;
use crate::voice::{Voice,VoiceError,VoiceResult};
use crate::Config;

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ElevenLabsVoiceCommand {
    options: Voice
}

impl TryFrom<Voice> for ElevenLabsVoiceCommand {
    type Error = ElevenLabsVoiceError;

    fn try_from(options: Voice) -> Result<Self, Self::Error> {
        Ok(ElevenLabsVoiceCommand { options })
    }
}

#[derive(Debug)]
pub enum ElevenLabsVoiceError {
    ValidationError(ElevenLabsValidationError),
    UnexpectedStatusCode(StatusCode)
}

impl ElevenLabsVoiceCommand {
    pub async fn run(&self, client: &Client, config: &Config) -> VoiceResult {
        let voice = self.options.voice.clone();
        let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{voice}");
        let overrides = ElevenLabsVoiceSettings::from(&self.options);
        let json = match (overrides.stability, overrides.similarity_boost) {
            (Some(stability), Some(similarity_boost)) => json!({
                "text": self.options.text.clone(),
                "voice_settings": {
                    "stability": stability,
                    "similarity_boost": similarity_boost
                }
            }),
            _ => json!({
                "text": self.options.text.clone()
            })
        };

        let request = client.post(url)
            .header("accept", HeaderValue::from_static("audio/mpeg"))
            .header("xi-api-key", &config.api_key_eleven_labs
                .as_ref()
                .map(|s| HeaderValue::from_str(&*s).unwrap())
                .ok_or_else(|| VoiceError::Unauthorized)?)
            .json(&json)
            .send()
            .await
            .expect("Failed to send voice request");

        match request.status() {
            StatusCode::UNAUTHORIZED => Err(VoiceError::Unauthorized),
            StatusCode::UNPROCESSABLE_ENTITY => {
                Err(ElevenLabsVoiceError::ValidationError(request.json().await?))?
            },
            StatusCode::OK => {
                let mut file = File::create(self.options.out.clone())?;
                let mut buffer = Vec::new();

                request.bytes().await?.iter().for_each(|b| buffer.push(*b));
                file.write_all(&buffer)?;
                Ok(())
            },
            code @ _ => Err(ElevenLabsVoiceError::UnexpectedStatusCode(code))?
        }
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ElevenLabsValidationError {
    detail: Vec<ElevenLabsValidationErrorDetail>
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ElevenLabsValidationErrorDetail {
    loc: Vec<serde_json::Value>,
    msg: String,
    r#type: String
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ElevenLabsVoiceSettings {
    stability: Option<usize>,
    similarity_boost: Option<usize>,
}

impl From<&Voice> for ElevenLabsVoiceSettings {
    fn from(voice: &Voice) -> Self {
        Self {
            stability: voice.voice_stability.clone(),
            similarity_boost: voice.voice_similarity_boost.clone()
        }
    }
}
