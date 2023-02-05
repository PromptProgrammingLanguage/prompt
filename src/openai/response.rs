use serde::Deserialize;

#[derive(Deserialize)]
#[serde(untagged)]
pub enum OpenAIResponse<T> {
    Ok(T),
    Err(OpenAIErrorResponse),
}

#[derive(Deserialize)]
pub struct OpenAIErrorResponse {
    pub error: OpenAIError
}

#[derive(Deserialize, Debug)]
pub struct OpenAIError {
    pub message: String,
    pub r#type: String,
    pub param: Option<String>,
    pub code: Option<u32>
}
