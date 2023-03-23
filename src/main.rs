mod ast;
mod parser;
mod eval;
mod watch;

use clap::Parser;
use reqwest::{Client,ClientBuilder,header::HeaderMap,header::HeaderValue};
use eval::{Evaluate,EvaluateConfig};
use std::fs;
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

    let config = EvaluateConfig {
        api_key,
        prompt_dir: args.path.parent()
            .expect("Prompt file must have a parent directory")
            .to_path_buf(),
        prompt_path: args.path,
    };

    if !config.prompt_path.is_file() {
        panic!("prompt path is not a file");
    }

    let file = fs::read_to_string(&config.prompt_path)
        .expect(&format!("Failed to open {}", "fooabr"));

    let program = parser::parse::program(&file)
        .expect("Couldn't parse the prompt program correctly")
        .expect("Couldn't parse the prompt program correctly");

    let main_name = program.prompts.iter()
        .find(|prompt| prompt.is_main)
        .map(|m| m.name.clone())
        .unwrap();

    let session_dir = config.prompt_dir.join("sessions");

    fs::create_dir_all(&session_dir)
        .expect("A sessions directory could not be created");

    let watched = session_dir.join(main_name);

    tokio::spawn(async move {
        let eval = Evaluate::new(client, program, config);
        eval.eval().await;
    });

    watch::monitor(watched).await.unwrap();
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct PromptArgs {
    /// Path to the main prompt file
    path: PathBuf
}
