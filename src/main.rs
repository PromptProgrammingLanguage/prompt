use std::fs;
use std::concat;
use clap::{Parser,Subcommand};
use reqwest::ClientBuilder;
use reqwest::header::{HeaderValue,HeaderMap};
use dirs;
use ai::{
    DEFAULT_CONFIG_FILE,
    ChatCommand,
    Config,
    ImageCommand,
    JSONConfig,
    PictureFormat,
    SessionCommand,
    VoiceCommand
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let config_dir = dirs::config_dir()
        .map(|mut path| {
            path.push("ai");
            path
        })
        .expect("Configuration directory could not be found");

    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)
            .expect("Could not create the configuration directory");
    }

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
        api_key_cohere: config_json.api_key_cohere,
        api_key_openai: config_json.api_key_openai,
        api_key_eleven_labs: config_json.api_key_eleven_labs,
        dir: config_dir,
        proxy: cli.proxy
    };

    let mut headers = HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_static("application/json"));
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));

    let client = ClientBuilder::new()
        .default_headers(headers)
        .build()
        .expect("Failed to construct http client");

    match cli.command {
        Commands::Chat(chat) => {
            let result = chat.run(&client, &config).await;
            if let Err(e) = result {
                eprintln!("{:#?}", e);
            }
        },
        Commands::Session(session) => {
            let result = session.run(&client, &config).await;
            if let Err(e) = result {
                eprintln!("{:#?}", e);
            }
        },
        Commands::Image(image) => {
            let result = image.run(&client, &config).await;

            match (result, image.out, image.format) {
                (Ok(result), None, _p @ PictureFormat::Url) => {
                    println!("{}", serde_json::to_string(&result)
                        .expect(&concat!(
                            "Image response could not be serialized to JSON. Did the AI providers ",
                            "API change?")));
                },
                (Err(e), _, _) => {
                    eprintln!("{:#?}", e);
                },
                _ => {}
            }
        },
        Commands::Voice(voice) => {
            let result = voice.command.run(&client, &config).await;
            if let Err(e) = result {
                eprintln!("{:#?}", e);
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

    #[arg(long)]
    proxy: Option<String>
}

#[derive(Subcommand)]
enum Commands {
    /// Starts (or resumes) a chat session
    Chat(ChatCommand),

    /// Starts a prompt based session
    Session(SessionCommand),

    /// Generates an image
    Image(ImageCommand),

    /// Translates text to a character voice
    Voice(VoiceCommand),
}
