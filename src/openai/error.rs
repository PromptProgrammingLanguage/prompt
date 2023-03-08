use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct OpenAIError {
    pub error: OpenAIErrorInner
}

#[derive(Deserialize, Debug, Clone)]
pub struct OpenAIErrorInner {
    pub message: String,
    pub r#type: String,
    pub param: Option<String>,
    pub code: Option<String>
}
