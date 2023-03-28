use clap::Args;
use serde::{Serialize,Deserialize};
use derive_more::From;
use std::path::PathBuf;
use reqwest::{Client};
use crate::eleven_labs::voice::{ElevenLabsVoiceCommand,ElevenLabsVoiceError};
use crate::Config;

#[derive(Args, Clone, Default, Debug, Serialize, Deserialize)]
pub struct Voice {
    /// The text to transcribe to a voice
    pub text: String,

    #[arg(long, short)]
    pub out: PathBuf,

    #[arg(long, short)]
    pub voice: String,

    #[arg(long)]
    pub voice_stability: Option<usize>,

    #[arg(long)]
    pub voice_similarity_boost: Option<usize>,
}

impl Voice {
    pub async fn run(&self, client: &Client, config: &Config) -> VoiceResult {
        let command = ElevenLabsVoiceCommand::try_from(self.clone())?;
        Ok(command.run(client, &config).await?)
    }
}

#[derive(Debug, From)]
pub enum VoiceError {
    ElevenLabsVoiceError(ElevenLabsVoiceError),
    NetworkError(reqwest::Error),
    IOError(std::io::Error),
    Unauthorized
}

pub type VoiceResult = Result<(), VoiceError>;
