use std::fs;
use std::path::PathBuf;
use std::fs::{File,OpenOptions};
use std::io::Write;
use clap::{Args,ValueEnum};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;
use crate::openai::response::OpenAIResponse;

#[derive(Args, Default)]
pub struct SessionCommand {
    /// Append a new input to an existing session and get only the latest response
    #[arg(long, short)]
    pub append: Option<String>,

    /// Model to use
    #[arg(value_enum, long, short, default_value_t = Model::TextDavinci)]
    pub model: Model,

    /// Temperature of the model on a scale from 0 to 1. 0 is most accurate while 1 is most creative
    #[arg(long, short, default_value_t = 0.0)]
    pub temperature: f32,

    /// Saves your conversation context using the session name
    #[arg(short, long)]
    pub session: Option<String>,

    /// Running conversation wrapper to assist the AI in responding. The current conversation can be
    /// inserted into the wrapper using the {{TRANSCRIPT}} variable. Run ai with the
    /// --print-default-wrappers flag to see examples of what's used for the text and code models.
    #[arg(long)]
    pub wrapper: Option<String>,

    /// Disables the context of the conversation, every message sent to the AI is standalone. If you
    /// use a coding model this defaults to true unless wrapper is specified.
    #[arg(long)]
    pub no_context: Option<bool>,
    
    /// Lists the default wrappers for chat models. Useful if you want to start with a template when
    /// writing your own wrapper.
    #[arg(long, default_value_t = false)]
    pub print_default_wrappers: bool,
}

pub type SessionResult = Result<Vec<String>, SessionError>;

pub enum SessionError {
    AppendRequiresSession
}

impl SessionCommand {
    pub fn run(&self, client: Client, config_dir: PathBuf) {

        if append.is_some() && session.is_none() {
            return Result::Err(SessionError::AppendRequiresSession);
        }

        let no_context = self.no_context.unwrap_or_else(|| {
            match model {
                Model::TextDavinci |
                Model::TextCurie |
                Model::TextBabbage |
                Model::TextAda => false,
                Model::CodeDavinci |
                Model::CodeCushman => true
            }
        });
        let wrapper = self.wrapper.clone().unwrap_or_else(|| {
            match model {
                Model::TextDavinci |
                Model::TextCurie |
                Model::TextBabbage |
                Model::TextAda => DEFAULT_CHAT_PROMPT_WRAPPER.to_owned(),
                Model::CodeDavinci |
                Model::CodeCushman => DEFAULT_CODE_PROMPT_WRAPPER.to_owned()
            }
        });
        let session_dir = {
            let mut dir = config_dir.clone();
            dir.push("sessions");
            dir
        };
        fs::create_dir_all(&session_dir).expect("Config directory could not be created");
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

        if self.print_default_wrappers {
            print_default_wrappers();
            return Result::Ok(vec![]);
        }

        print_opening_prompt(&self, &current_transcript);

        let mut line = append.clone().or_else(|| read_next_user_line());
        match line {
            Some(ref line) => write_next_line(&line, &mut current_transcript, session_file.as_mut()),
            None => return Result::Ok(vec![]),
        }

        loop {
            let prompt = if no_context {
                line.unwrap()
            } else {
                wrapper.replace("{{TRANSCRIPT}}", &current_transcript)
            };

            let res = client.post("https://api.openai.com/v1/completions")
                .json(&json!({
                    "model": model.to_versioned(), 
                    "prompt": &prompt,
                    "max_tokens": 1000,
                    "temperature": temperature
                }))
                .send()
                .expect("Failed to send completion");

            let response: OpenAIResponse::<Response> = res.json().expect("Unknown json response from OpenAI");

            match response {
                OpenAIResponse::Ok(r) => {
                    let text = &r.choices.first().unwrap().text;
                    write_next_line(text, &mut current_transcript, session_file.as_mut());
                    println!("{}", text);

                    if append.is_some() {
                        return Result::Ok(vec![ text.to_owned() ]);
                    }
                },
                OpenAIResponse::Err(err) => {
                    eprintln!("Error: {:?}", err.error);
                }
            }

            line = read_next_user_line();
            match line {
                Some(ref line) => write_next_line(&line, &mut current_transcript, session_file.as_mut()),
                None => return Result::Ok(vec![]),
            }
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Model {
    #[default]
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

const DEFAULT_CODE_PROMPT_WRAPPER: &str = "{{TRANSCRIPT}}";
const DEFAULT_CHAT_PROMPT_WRAPPER: &str = "
The following is a transcript between a helpful AI assistant and a human. The AI assistant can provide factual information (but only from before mid 2021, when its training data cuts off), ask clarifying questions, and engage in chit chat.

Transcript:

{{TRANSCRIPT}}

Output the next thing the AI says:
";

#[derive(Deserialize)]
pub struct Response {
    choices: Vec<ResponseChoice>,
}

#[derive(Deserialize)]
pub struct ResponseChoice {
    pub text: String,
    pub index: u32,
    pub logprobs: Option<u32>,
    pub finish_reason: String
}

#[derive(Deserialize)]
pub struct ResponseUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32
}

fn print_opening_prompt(args: &SessionCommand, session_file: &str) {
    if args.append.is_some() {
        return;
    }

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

fn print_default_wrappers() {
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
