use serde::Deserialize;

#[derive(Deserialize)]
pub struct OpenAIErrorResponse {
    pub error: OpenAIError
}

#[derive(Deserialize, Debug, Clone)]
pub struct OpenAIError {
    pub message: String,
    pub r#type: String,
    pub param: Option<String>,
    pub code: Option<u32>
}
