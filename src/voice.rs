use clap::{Args,Subcommand};
use serde::{Serialize,Deserialize};
use derive_more::From;
use std::path::PathBuf;
use reqwest::{Client};
use crate::eleven_labs::voice::{ElevenLabsListCommand,ElevenLabsGenerateCommand,ElevenLabsVoiceError};
use crate::Config;

#[derive(Args)]
pub struct VoiceCommand {
    #[command(subcommand)]
    pub command: Voice,
}

#[derive(Subcommand)]
pub enum Voice {
    List(VoiceList),
    Generate(VoiceGenerate)
}

#[derive(Args, Clone, Default, Debug, Serialize, Deserialize)]
pub struct VoiceList {
    #[arg(long, short, default_value_t = false)]
    pub verbose: bool,

    #[arg(long, short, default_value_t = false)]
    pub quiet: bool
}

#[derive(Args, Clone, Default, Debug, Serialize, Deserialize)]
pub struct VoiceGenerate {
    /// The text to transcribe to a voice
    pub text: String,

    #[arg(long, short)]
    pub out: PathBuf,

    /// The name of the voice to use, run the list command to see your options
    #[arg(long, short)]
    pub voice: String,

    #[arg(long)]
    pub voice_stability: Option<usize>,

    #[arg(long)]
    pub voice_similarity_boost: Option<usize>,
}

impl Voice {
    pub async fn run(&self, client: &Client, config: &Config) -> VoiceResult {
        match self {
            Self::List(list) => {
                let command = ElevenLabsListCommand::try_from(list.clone())?;
                command.run(client, &config).await?;
                Ok(())
            },
            Self::Generate(generate) => {
                let command = ElevenLabsGenerateCommand::try_from(generate.clone())?;
                command.run(client, &config).await?;
                Ok(())
            }
        }
    }
}

#[derive(Debug, From)]
pub enum VoiceError {
    InvalidArguments(String),
    ElevenLabsVoiceError(ElevenLabsVoiceError),
    NetworkError(reqwest::Error),
    IOError(std::io::Error),
    Serde(serde_json::Error),
    Unauthorized
}

pub type VoiceResult = Result<(), VoiceError>;
