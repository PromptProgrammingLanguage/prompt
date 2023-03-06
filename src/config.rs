use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize)]
pub struct JSONConfig {
    pub api_key: Option<String>,
    pub api_key_cohere: Option<String>,
    pub api_key_openai: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub api_key: Option<String>,
    pub api_key_cohere: Option<String>,
    pub api_key_openai: Option<String>,
    pub dir: PathBuf
}

pub const DEFAULT_CONFIG_FILE: &str = r#"{
    "api_key": "",
    "api_key_cohere": "",
    "api_key_openai": ""
}"#;
