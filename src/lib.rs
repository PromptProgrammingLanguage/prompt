pub mod ast;
pub mod parser;
pub mod eval;
pub mod watch;

use clap::Parser;
use reqwest::{ClientBuilder,header::HeaderMap,header::HeaderValue};
use eval::{Evaluate,EvaluateError,EvaluateConfig};
use std::fs;
use std::env;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct PromptArgs {
    /// Path to the main prompt file
    pub path: PathBuf,

    /// What session file to watch, if any
    #[arg(long, short)]
    pub watch: Option<String>,

    /// By default, we output the result of commands in a final position to stdout (the terminal).
    /// This will suppress that output.
    #[arg(long, short, default_value_t = false)]
    pub quiet: bool
}

pub async fn prompt(args: PromptArgs) {
    let api_key = env::var("AI_API_KEY")
        .expect("AI_API_KEY environment variable is missing");

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
        quiet: args.quiet
    };

    if !config.prompt_path.is_file() {
        panic!("prompt path is not a file");
    }

    let file = fs::read_to_string(&config.prompt_path)
        .expect(&format!("Failed to open {}", "fooabr"));

    let program = parser::parse::program(&file)
        .expect("Couldn't parse the prompt program correctly")
        .expect("Couldn't parse the prompt program correctly");

    let session_dir = config.prompt_dir.join("sessions");
    let prompts = program.prompts.clone();

    fs::create_dir_all(&session_dir)
        .expect("A sessions directory could not be created");

    let eval = tokio::spawn(async move {
        let eval = Evaluate::new(client, program, config);
        if let Err(e) = eval.eval().await {
            match e {
                EvaluateError::CommandExited => std::process::exit(0),
                _ => eprintln!("{:#?}", e)
            }
        }
    });

    match args.watch {
        Some(watch) => {
            let watched_exists = prompts.iter().find(|prompt| prompt.name == watch).is_some();
            if !watched_exists {
                eprintln!("Invalid watch {watch}");
                std::process::exit(0);
            }
            let watched = session_dir.join(watch);
            watch::monitor(watched).await.unwrap();
        },
        None => {
            eval.await;
        }
    }

}
