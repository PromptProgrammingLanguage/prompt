use std::fs;
use std::path::PathBuf;
use clap::{Args,ValueEnum};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use rustc_serialize::base64::FromBase64;
use crate::openai::response::OpenAIResponse;

#[derive(Args)]
pub struct ImageCommand {
    /// Description of the image
    #[arg(long, short)]
    prompt: String,

    /// Number of images generated
    #[arg(long, short, default_value_t = 1)]
    count: usize,

    /// Generated image size
    #[arg(value_enum, long, short, default_value_t = PictureSize::x512)]
    size: PictureSize,

    /// Format of the response
    #[arg(value_enum, long, short, default_value_t = PictureFormat::Url)]
    format: PictureFormat,

    /// Directory to output files
    #[arg(value_enum, long, short)]
    out: Option<PathBuf>,
}

impl ImageCommand {
    pub async fn run(&self, client: Client, _config_dir: PathBuf, config: Config) {
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

        match response {
            OpenAIResponse::Ok(response) => {
                match self.out {
                    Some(ref out) => {
                        fs::create_dir_all(&out)
                            .expect(r#"Image "out" directory could not be created"#);

                        for (i, data) in response.data.iter().enumerate() {
                            match data {
                                OpenAIImageData::Url(_) => unreachable!(
                                    "Response data should be in binary format"),

                                OpenAIImageData::Binary(data) => {
                                    let content = data.b64_json.from_base64().unwrap();
                                    let mut path = out.clone();
                                    path.push(format!("{}.png", i));

                                    fs::write(path, content).unwrap();
                                }
                            }
                        }
                    },
                    None => {}
                }
            },
            OpenAIResponse::Err(_) => {}
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct OpenAIImageResponse {
    pub created: usize,
    pub data: Vec<OpenAIImageData>
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum OpenAIImageData {
    Url(OpenAIImageUrl),
    Binary(OpenAIImageBinary),
}

#[derive(Deserialize, Debug)]
pub struct OpenAIImageUrl {
    pub url: String
}

#[derive(Deserialize, Debug)]
pub struct OpenAIImageBinary {
    pub b64_json: String
}

#[derive(Copy, Clone, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
#[allow(non_camel_case_types)]
enum PictureSize {
    x256,
    x512,
    x1024
}

#[derive(Copy, Clone, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum PictureFormat {
    Url,
    Binary
}
