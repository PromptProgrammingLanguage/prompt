use std::io::Write;
use std::fs::{self,File};
use serde::{Serialize,Deserialize};
use serde_json::json;
use reqwest::{Client,StatusCode};
use reqwest::header::HeaderValue;
use crate::voice::{VoiceGenerate,VoiceList,VoiceError,VoiceResult};
use crate::Config;

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ElevenLabsGenerateCommand {
    options: VoiceGenerate
}

impl TryFrom<VoiceGenerate> for ElevenLabsGenerateCommand {
    type Error = ElevenLabsVoiceError;

    fn try_from(options: VoiceGenerate) -> Result<Self, Self::Error> {
        Ok(Self { options })
    }
}

#[derive(Debug)]
pub enum ElevenLabsVoiceError {
    ValidationError(ElevenLabsValidationError),
    UnexpectedStatusCode(StatusCode)
}

impl ElevenLabsGenerateCommand {
    pub async fn run(&self, client: &Client, config: &Config) -> VoiceResult {
        let voice = self.options.voice.clone();
        let mut used_cache = false;
        let voices = match fs::read_to_string(config.dir.join("eleven_labs_voices.json")) {
            Ok(json) => match serde_json::from_str(&json) {
                Ok(existing) => {
                    used_cache = true;
                    existing
                },
                _ => ElevenLabsListCommand::quiet().run(client, config).await?
            },
            _ => ElevenLabsListCommand::quiet().run(client, config).await?
        };


        let voice_id = voices
            .voices
            .into_iter()
            .find(|v| v.voice_id == voice || v.name == voice)
            .map(|v| v.voice_id);

        let voice_id = match (voice_id, used_cache) {
            (Some(id), _) => Some(id),
            (None, true) => {
                let voices = ElevenLabsListCommand::quiet().run(client, config).await?;
                voices
                    .voices
                    .into_iter()
                    .find(|v| v.voice_id == voice || v.name == voice)
                    .map(|v| v.voice_id)
            },
            (None, false) => None,
        };

        let voice_id = voice_id.ok_or_else(|| VoiceError::InvalidArguments(format!(
            r#"Could not find voice id for {voice}, try listing the available voices with list"#
        )))?;

        let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{voice_id}");

        let json = match (self.options.voice_stability, self.options.voice_similarity_boost) {
            (Some(stability), Some(similarity_boost)) => json!({
                "text": self.options.text.clone(),
                "voice_settings": {
                    "stability": stability,
                    "similarity_boost": similarity_boost
                }
            }),
            (Some(_), None) |
            (None, Some(_)) => return Err(VoiceError::InvalidArguments(
                String::from(concat!(
                    "If you specify an override for stability or similarity_boost in Eleven Labs ",
                    "then you need to specify them both. You don't need to specify either though ",
                    "because they're saved in your account as defaults on the voice."
                ))
            )),
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
pub struct ElevenLabsListCommand {
    options: VoiceList
}

impl TryFrom<VoiceList> for ElevenLabsListCommand {
    type Error = VoiceError;

    fn try_from(options: VoiceList) -> Result<Self, Self::Error> {
        if options.verbose && options.quiet {
            return Err(VoiceError::InvalidArguments(
                String::from("Cannot be both verbose and quiet")
            ))
        }
        Ok(Self { options })
    }
}

impl ElevenLabsListCommand {
    pub fn quiet() -> Self {
        Self {
            options: VoiceList { verbose: false, quiet: true }
        }
    }

    pub async fn run(
        &self,
        client: &Client,
        config: &Config) -> Result<ElevenLabsGetVoicesResponseModel, VoiceError>
{
        let request = client.get("https://api.elevenlabs.io/v1/voices")
            .header("xi-api-key", &config.api_key_eleven_labs
                .as_ref()
                .map(|s| HeaderValue::from_str(&*s).unwrap())
                .ok_or_else(|| VoiceError::Unauthorized)?)
            .send()
            .await
            .expect("Failed to send voice request");

        match request.status() {
            StatusCode::UNAUTHORIZED => Err(VoiceError::Unauthorized),
            StatusCode::UNPROCESSABLE_ENTITY => {
                Err(ElevenLabsVoiceError::ValidationError(request.json().await?))?
            },
            StatusCode::OK => {
                let voices: ElevenLabsGetVoicesResponseModel = request.json().await?;
                fs::write(
                    config.dir.join("eleven_labs_voices.json"),
                    &serde_json::to_string(&voices)?)?;

                if !self.options.quiet {
                    if self.options.verbose {
                        println!("{:#?}", voices);
                    } else {
                        println!("{}", voices.voices.iter()
                            .map(|voice| voice.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                        );
                    }
                }
                Ok(voices)
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevenLabsGetVoicesResponseModel {
    voices: Vec<ElevenLabsVoiceResponseModel>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevenLabsVoiceResponseModel {
    voice_id: String,
    name: String,
    samples: Option<Vec<ElevenLabsSampleResponseModel>>,
    category: String,
    fine_tuning: ElevenLabsFineTuningResponseModel,
    labels: Option<serde_json::Value>,
    preview_url: String,
    available_for_tiers: Vec<String>,
    settings: Option<ElevenLabsVoiceSettingsResponseModel>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevenLabsSampleResponseModel {
    sample_id: String,
    file_name: String,
    mime_type: String,
    size_bytes: i32,
    hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevenLabsFineTuningResponseModel {
    model_id: Option<String>,
    is_allowed_to_fine_tune: bool,
    fine_tuning_requested: bool,
    finetuning_state: String,
    verification_attempts: Option<Vec<ElevenLabsVerificationAttemptResponseModel>>,
    verification_failures: Vec<String>,
    verification_attempts_count: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevenLabsVerificationAttemptResponseModel {
    text: String,
    date_unix: i32,
    accepted: bool,
    similarity: f64,
    levenshtein_distance: f64,
    recording: ElevenLabsRecordingResponseModel,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevenLabsRecordingResponseModel {
    recording_id: String,
    mime_type: String,
    size_bytes: i32,
    upload_date_unix: i32,
    transcription: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevenLabsVoiceSettingsResponseModel {
    stability: f64,
    similarity_boost: f64,
}
