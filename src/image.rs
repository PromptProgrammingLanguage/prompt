use std::fs;
use std::path::PathBuf;
use clap::{Args,ValueEnum};
use reqwest::Client;
use serde::{Deserialize,Serialize};
use serde_json::json;
use rustc_serialize::base64::FromBase64;
use derive_more::{From,TryInto};
use crate::openai::response::{OpenAIResponse,OpenAIError};

#[derive(Clone, Debug, Args)]
pub struct ImageCommand {
    /// Description of the image
    #[arg(long, short)]
    pub prompt: String,

    /// Number of images generated
    #[arg(long, short, default_value_t = ImageCommand::default().count)]
    pub count: usize,

    /// Generated image size
    #[arg(value_enum, long, short, default_value_t = PictureSize::default())]
    pub size: PictureSize,

    /// Format of the response
    #[arg(value_enum, long, short, default_value_t = PictureFormat::default())]
    pub format: PictureFormat,

    /// Directory to output files
    #[arg(value_enum, long, short)]
    pub out: Option<PathBuf>,
}

impl Default for ImageCommand {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            count: 1,
            size: PictureSize::default(),
            format: PictureFormat::default(),
            out: None
        }
    }
}

pub type ImageResult = Result<Vec<ImageData>, ImageError>;

#[derive(Clone, Debug, From, Serialize, Deserialize)]
pub enum ImageError {
    OpenAIError(OpenAIError),
    Hum
}

impl ImageCommand {
    pub async fn run(&self, client: &Client) -> ImageResult {
        let res = client.post("https://api.openai.com/v1/images/generations")
            .json(&json!({
                "prompt": &self.prompt,
                "n": self.count,
                "size": match self.size {
                    PictureSize::x256 => "256x256",
                    PictureSize::x512 => "512x512",
                    PictureSize::x1024 => "1024x1024",
                },
                "response_format": match &self.out {
                    Some(_) => "b64_json",
                    None => match self.format {
                        PictureFormat::Url => "url",
                        PictureFormat::Binary => "b64_json"
                    }
                }
            }))
            .send()
            .await
            .expect("Failed to send completion");

        let response: OpenAIResponse::<OpenAIImageResponse> = res.json()
            .await
            .expect("Unknown json response from OpenAI");

        match (&self.out, response) {
            (Some(out), OpenAIResponse::Ok(response)) => {
                write_data_to_directory(out, &response);
                Ok(response.data)
            },
            (None, OpenAIResponse::Ok(response)) => Ok(response.data),
            (_, OpenAIResponse::Err(e)) => Err(e.error.into())
        }
    }
}

fn write_data_to_directory(out: &PathBuf, response: &OpenAIImageResponse) {
    fs::create_dir_all(&out)
        .expect(r#"Image "out" directory could not be created"#);

    for (i, data) in response.data.iter().enumerate() {
        match data {
            ImageData::Url(_) => unreachable!(
                "Response data should be in binary format"),

            ImageData::Binary(data) => {
                let content = data.b64_json.from_base64().unwrap();
                let mut path = out.clone();
                path.push(format!("{}.png", i));

                fs::write(path, content).unwrap();
            }
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct OpenAIImageResponse {
    pub created: usize,
    pub data: Vec<ImageData>
}

#[derive(Clone, From, TryInto, Serialize, Deserialize, Debug)]
#[serde(untagged)]
#[try_into(owned, ref, ref_mut)]
pub enum ImageData {
    Url(ImageUrl),
    Binary(ImageBinary),
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct ImageUrl {
    pub url: String
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ImageBinary {
    pub b64_json: String
}

#[derive(Default, Copy, Clone, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
#[allow(non_camel_case_types)]
pub enum PictureSize {
    x256,
    #[default]
    x512,
    x1024
}

#[derive(Default, Copy, Clone, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum PictureFormat {
    #[default]
    Url,
    Binary
}
