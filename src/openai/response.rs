use serde::Deserialize;

#[derive(Deserialize)]
pub struct OpenAICompletionResponse<T> {
    pub choices: Vec<T>,
    pub created: usize,
    pub model: String,
    pub object: String,
    pub id: String,
    pub usage: Option<OpenAIUsage>
}

#[derive(Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize
}

#[derive(Deserialize)]
pub struct OpenAIChoice {
    pub text: String,
    pub index: u32,
    pub logprobs: Option<u32>,
    pub finish_reason: Option<String>
}
