use std::fs;
use std::io;
use std::fmt;
use std::env;
use std::concat;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::collections::HashMap;
use std::fs::{File,OpenOptions};
use std::io::{Read,BufReader};
use std::io::prelude::*;
use clap::{Parser,ValueEnum};
use serde::Deserialize;
use serde_json::json;
use dirs;

fn main() {
    let client = reqwest::blocking::ClientBuilder::new()
        .timeout(None)
        .build()
        .expect("Failed to construct http client");
    let args = Args::parse();
    let Args { model, temperature, print_default_wrappers, ref session, .. } = args;
    let config_dir = dirs::config_dir()
        .map(|mut path| {
            path.push("ai");
            path
        })
        .expect("Configuration directory could not be found");
    let session_dir = {
        let mut dir = config_dir.clone();
        dir.push("sessions");
        dir
    };

    fs::create_dir_all(&session_dir).expect("Config directory could not be created");

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

    let no_context = args.no_context.unwrap_or_else(|| {
        match model {
            Model::TextDavinci |
            Model::TextCurie |
            Model::TextBabbage |
            Model::TextAda => false,
            Model::CodeDavinci |
            Model::CodeCushman => true
        }
    });
    let wrapper = args.wrapper.clone().unwrap_or_else(|| {
        match model {
            Model::TextDavinci |
            Model::TextCurie |
            Model::TextBabbage |
            Model::TextAda => DEFAULT_CHAT_PROMPT_WRAPPER.to_owned(),
            Model::CodeDavinci |
            Model::CodeCushman => DEFAULT_CODE_PROMPT_WRAPPER.to_owned()
        }
    });
    let mut current_transcript = String::new();
    let mut session_file: Option<File> = if let Some(name) = session {
        let path = {
            let mut path = session_dir.clone();
            path.push(name);
            path
        };
        current_transcript = fs::read_to_string(&path).unwrap_or_default().trim_end().to_owned();

        Some(OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .expect("Unable to open session file path")
        )
    } else {
        None
    };

    if print_default_wrappers {
        println!(concat!(
            "\n",
            "The default wrapper for chat models is:\n",
            "----------------------------------------\n",
            "{}\n\n",
            "________________________________________\n\n",
            "And the default for code prompts is:\n",
            "----------------------------------\n\n",
            "{}\n"),
            DEFAULT_CHAT_PROMPT_WRAPPER, DEFAULT_CODE_PROMPT_WRAPPER);
        return;
    }

    print_opening_prompt(&args, &current_transcript);

    let mut line = read_next_user_line();
    match line {
        Some(ref line) => write_next_line(&line, &mut current_transcript, session_file.as_mut()),
        None => return
    }

    loop {
        let prompt = if no_context {
            line.unwrap()
        } else {
            wrapper.replace("{{TRANSCRIPT}}", &current_transcript)
        };

        let res = client.post("https://api.openai.com/v1/completions")
            .header("Content-Type", "application/json")
            .header("Authorization", "Bearer ".to_owned() + &api_key)
            .json(&json!({
                "model": model.to_versioned(), 
                "prompt": &prompt,
                "max_tokens": 1000,
                "temperature": temperature
            }))
            .send()
            .expect("Failed to send completion");

        let response: OpenAIResponse = res.json().expect("Unknown json response from OpenAI");

        match response {
            OpenAIResponse::Ok(r) => {
                let text = &r.choices.first().unwrap().text;
                write_next_line(text, &mut current_transcript, session_file.as_mut());
                println!("{}", text);
            },
            OpenAIResponse::Err(err) => {
                println!("Error: {:?}", err.error);
            }
        }

        line = read_next_user_line();
        match line {
            Some(ref line) => write_next_line(&line, &mut current_transcript, session_file.as_mut()),
            None => return
        }
    }
}

fn print_opening_prompt(args: &Args, session_file: &str) {
    if session_file.len() > 0 {
        println!("{}", session_file);
    } else {
        println!(concat!("\n",
            "Hello, I'm ChatGPT using the model: {} with a temperature of {}. ",
            "Ask me anything."),
            args.model.to_possible_value().unwrap().get_name(),
            args.temperature
        );
    }
}

fn read_next_user_line() -> Option<String> {
    let mut rl = rustyline::Editor::<()>::new().expect("Failed to create rusty line editor");
    match rl.readline("\n\t") {
        Ok(line) => Some(String::from("\n\t") + line.trim_end()),
        Err(_) => None
    }
}

fn write_next_line(line: &str, transcript: &mut String, mut session_file: Option<&mut File>) {
    if let Some(ref mut file) = session_file {
        if let Err(e) = writeln!(file, "{}", line) {
            eprintln!("Couldn't write to file: {}", e);
        }
    }
    *transcript += line;
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OpenAIResponse {
    Ok(Response),
    Err(OpenAIErrorResponse),
}

#[derive(Deserialize)]
struct OpenAIErrorResponse {
    error: OpenAIError
}

#[derive(Deserialize, Debug)]
struct OpenAIError {
    message: String,
    r#type: String,
    param: Option<String>,
    code: Option<u32>
}

#[derive(Deserialize)]

struct Response {
    choices: Vec<ResponseChoice>,
}

#[derive(Deserialize)]
struct ResponseChoice {
    text: String,
    index: u32,
    logprobs: Option<u32>,
    finish_reason: String
}

#[derive(Deserialize)]
struct ResponseUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32
}

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Model to use
    #[arg(value_enum, long, short, default_value_t = Model::TextDavinci)]
    model: Model,

    /// Temperature of the model on a scale from 0 to 1. 0 is most accurate while 1 is most creative
    #[arg(long, short, default_value_t = 0.0)]
    temperature: f32,

    /// Saves your conversation context using the session name
    #[arg(short, long)]
    session: Option<String>,

    /// Running conversation wrapper to assist the AI in responding. The current conversation can be
    /// inserted into the wrapper using the {{TRANSCRIPT}} variable. Run ai with the
    /// --print-default-wrappers flag to see examples of what's used for the text and code models.
    #[arg(long)]
    wrapper: Option<String>,

    /// Disables the context of the conversation, every message sent to the AI is standalone. If you
    /// use a coding model this defaults to true unless wrapper is specified.
    #[arg(long)]
    no_context: Option<bool>,
    
    /// Lists the default wrappers for chat models. Useful if you want to start with a template when
    /// writing your own wrapper.
    #[arg(long, default_value_t = false)]
    print_default_wrappers: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct Config {
    api_key: String
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Model {
    TextDavinci,
    TextCurie,
    TextBabbage,
    TextAda,
    CodeDavinci,
    CodeCushman
}

impl Model {
    fn to_versioned(&self) -> &str {
        match self {
            Model::TextDavinci => "text-davinci-003",
            Model::TextCurie => "text-curie-001",
            Model::TextBabbage => "text-babbage-001",
            Model::TextAda => "text-ada-001",
            Model::CodeDavinci => "code-davinci-002",
            Model::CodeCushman => "code-cushman-001",
        }
    }
}

const DEFAULT_CONFIG_FILE: &str = r#"{
    "api_key": ""
}"#;
const DEFAULT_CODE_PROMPT_WRAPPER: &str = "{{TRANSCRIPT}}";
const DEFAULT_CHAT_PROMPT_WRAPPER: &str = "
The following is a transcript between a helpful AI assistant and a human. The AI assistant can provide factual information (but only from before mid 2021, when its training data cuts off), ask clarifying questions, and engage in chit chat.

Transcript:

{{TRANSCRIPT}}

Output the next thing the AI says:
";
