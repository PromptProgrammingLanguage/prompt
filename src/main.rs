mod image;
mod session;
mod openai;

use std::fs;
use std::env;
use std::concat;
use clap::{Parser,Subcommand};
use serde::Deserialize;
use reqwest::blocking::{ClientBuilder};
use reqwest::header::{HeaderValue,HeaderMap};
use dirs;
use image::ImageCommand;
use session::SessionCommand;

fn main() {
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

    let config: Config = serde_json::from_str(&config_string)
        .expect("Config file could not be read");

    let api_key = env::var("AI_API_KEY")
        .unwrap_or_else(|_| config.api_key);

    if api_key.len() == 0 {
        panic!(concat!(
            "An API key needs to be passed as either the AI_API_KEY environment varaible or ",
            "specified as the api_key in the config file found at: {}"), &config_file.display());
    }

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    headers.insert("Authorization", HeaderValue::from_str(&("Bearer ".to_owned() + &api_key)).unwrap());

    let client = ClientBuilder::new()
        .default_headers(headers)
        .timeout(None)
        .build()
        .expect("Failed to construct http client");

    match cli.command {
        Commands::Session(session) => session.run(client, config_dir),
        Commands::Image(image) => image.run(client, config_dir),
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

#[derive(Clone, Debug, Deserialize)]
struct Config {
    api_key: String
}

const DEFAULT_CONFIG_FILE: &str = r#"{
    "api_key": ""
}"#;
