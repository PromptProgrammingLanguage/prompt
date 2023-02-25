extern crate derive_more;

mod image;
mod session;
mod openai;
mod cohere;
mod config;

pub use config::Config;

use std::fs;
use std::env;
use std::concat;
use clap::{Parser,Subcommand};
use reqwest::ClientBuilder;
use reqwest::header::{HeaderValue,HeaderMap};
use dirs;
use image::{ImageCommand,PictureFormat};
use session::SessionCommand;
use config::{JSONConfig,DEFAULT_CONFIG_FILE};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let config_dir = dirs::config_dir()
        .map(|mut path| {
            path.push("ai");
            path
        })
        .expect("Configuration directory could not be found");

    let config_file = {
        let mut config_file = config_dir.clone();
        config_file.push("config.json");
        config_file
    };

    if !config_file.exists() {
        fs::write(&config_file, DEFAULT_CONFIG_FILE)
            .expect(&format!("Default config file could not be written to {}", &config_file.display()));
    }

    let config_string = fs::read_to_string(&config_file)
        .unwrap_or_else(|_| DEFAULT_CONFIG_FILE.into());

    let config_json: JSONConfig = serde_json::from_str(&config_string)
        .expect("Config file could not be read");

    let config = Config {
        api_key: config_json.api_key,
        api_key_cohere: config_json.api_key_cohere,
        api_key_openai: config_json.api_key_openai,
        dir: config_dir
    };

    let mut headers = HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_static("application/json"));
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));

    if let Some(key) = env::var("AI_API_KEY").ok().or_else(|| config.api_key.clone()) {
        let bearer = "Bearer ".to_owned() + &key;
        headers.insert("Authorization", HeaderValue::from_str(&bearer).unwrap());
    }

    let client = ClientBuilder::new()
        .default_headers(headers)
        .build()
        .expect("Failed to construct http client");

    match cli.command {
        Commands::Session(mut session) => {
            let result = session.run(&client, &config).await;
            if let Err(e) = result {
                eprintln!("{:?}", e);
            }
        },
        Commands::Image(image) => {
            let result = image.run(&client).await;

            match (result, image.out, image.format) {
                (Ok(result), None, _p @ PictureFormat::Url) => {
                    println!("{}", serde_json::to_string(&result)
                        .expect(&concat!(
                            "Image response could not be serialized to JSON. Did the AI providers ",
                            "API change?")));
                },
                (Err(e), _, _) => {
                    eprintln!("{:?}", e);
                },
                _ => {}
            }
        }
    }
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Starts a chat session
    Session(SessionCommand),

    /// Generates an image
    Image(ImageCommand)
}

