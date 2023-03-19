mod ast;
mod parser;
mod eval;

use clap::Parser;
use reqwest::{Client,ClientBuilder,header::HeaderMap,header::HeaderValue};
use eval::{Evaluator,EvaluatorConfig};
use std::env;
use std::path::PathBuf;
use std::process::Command;

#[tokio::main]
async fn main() {
    let api_key = env::var("AI_API_KEY")
        .expect("AI_API_KEY environment variable is missing");

    let args = PromptArgs::parse();

    let client = {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        headers.insert("Authorization", HeaderValue::from_str(&("Bearer ".to_owned() + &api_key)).unwrap());

        ClientBuilder::new()
            .default_headers(headers)
            .build()
            .expect("Failed to construct http client")
    };

    let eval = Evaluator {
        client,
        config: EvaluatorConfig {
            api_key,
            prompt_path: args.path,
        }
    };

    eval.eval().await;
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct PromptArgs {
    /// Path to the main prompt file
    path: PathBuf
}
