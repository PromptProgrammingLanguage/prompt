use prompt::{PromptArgs,prompt};
use clap::Parser;

#[tokio::main]
async fn main() {
    let args = PromptArgs::parse();
    prompt(args).await;
}
