use std::fs;
use std::path::PathBuf;
use std::fs::{File,OpenOptions};
use std::io::Write;
use clap::{Args,ValueEnum};
use reqwest::Client;
use serde::{Serialize,Deserialize};
use crate::openai::session::{OpenAISessionCommand};
use crate::openai::response::OpenAIError;
use crate::cohere::session::{CohereSessionCommand,CohereError};
use crate::Config;

#[derive(Args, Clone, Debug, Serialize, Deserialize)]
pub struct SessionCommand {
    /// Allow the AI to generate a response to the prompt before user input
    #[arg(long)]
    pub ai_responds_first: bool,

    /// Append a new input to an existing session and get only the latest response. If
    /// ai-responds-first is set to true then only the ai response is included.
    #[arg(long)]
    pub append: bool,

    /// Append a string to an existing session and get only the latest response. This is the same as
    /// the "append" cli argument but it takes a string directly instead of blocking to wait for
    /// user input.
    #[arg(long)]
    pub append_string: Option<String>,

    /// Model size
    #[arg(value_enum, long, short, default_value_t = SessionCommand::default().model)]
    pub model: Model,

    /// Model focus
    #[arg(value_enum, long, default_value_t = SessionCommand::default().model_focus)]
    pub model_focus: ModelFocus,

    /// Temperature of the model on a scale from 0 to 1. 0 is most accurate while 1 is most creative
    #[arg(long, short, default_value_t = SessionCommand::default().temperature)]
    pub temperature: f32,

    /// Saves your conversation context using the session name
    #[arg(short, long)]
    pub name: Option<String>,

    /// Overwrite the existing session if it already exists
    #[arg(long)]
    pub overwrite: bool,

    /// Override the existing session configuration
    #[arg(long)]
    pub override_session_configuration: bool,

    /// Running conversation prompt to assist the AI in responding. The current conversation can be
    /// inserted into the prompt using the ${TRANSCRIPT} variable. Run ai with the
    /// --print-default-prompts flag to see examples of what's used for the text and code models.
    #[arg(long)]
    pub prompt: Option<String>,

    /// Path to a prompt file to load. See prompt option for details.
    #[arg(long)]
    pub prompt_path: Option<PathBuf>,

    /// Disables the context of the conversation, every message sent to the AI is standalone. If you
    /// use a coding model this defaults to true unless prompt is specified.
    #[arg(long)]
    pub no_context: Option<bool>,
    
    /// Lists the default prompts for chat models. Useful if you want to start with a template when
    /// writing your own prompt.
    #[arg(long)]
    pub print_default_prompts: bool,

    /// Only write output the session file
    #[arg(long)]
    pub quiet: bool,

    /// Provider for the session service
    #[arg(value_enum, long, default_value_t = SessionCommand::default().provider)]
    pub provider: Provider,

    /// Prefix input with the supplied string. This can be used for labels if your prompt has a
    /// conversational style. If you're using the default chat prompt then this defaults to
    /// "HUMAN: ", otherwise it's an empty string.
    #[arg(long)]
    pub prefix_user: Option<String>,

    /// Prefix ai responses with the supplied string. This can be used for labels if your prompt has
    /// a conversational style. If you're using the default chat prompt then this defaults to
    /// "AI: ", otherwise it's an empty string.
    #[arg(long)]
    pub prefix_ai: Option<String>,

    /// Number of responses to generate
    #[arg(skip)]
    pub response_count: Option<usize>,
}

impl Default for SessionCommand {
    fn default() -> Self {
        SessionCommand {
            ai_responds_first: false,
            append: false,
            append_string: None,
            model: Model::default(),
            model_focus: ModelFocus::default(),
            temperature: 0.8,
            name: None,
            overwrite: false,
            override_session_configuration: false,
            prompt: None,
            prompt_path: None,
            no_context: None,
            print_default_prompts: false,
            response_count: None,
            quiet: false,
            provider: Provider::default(),
            prefix_ai: None,
            prefix_user: None,
        }
    }
}


pub type SessionResult = Result<Vec<String>, SessionError>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionError {
    AppendRequiresSession,
    AppendsClash,
    AppendAndAiRespondsFirstIsNonsensical,
    QuietRequiresSession,
    NoMatchingModel,
    OverwriteRequiresSession,
    TemperatureOutOfValidRange,
    ZeroResponseCountIsNonsensical,
    CohereError(CohereError),
    OpenAIError(OpenAIError)
}

impl SessionCommand {
    pub async fn run(&mut self, client: &Client, config: &Config) -> SessionResult {
        let SessionCommand { ref name, .. } = self;

        self.validate()?;

        self.no_context = Some(self.parse_no_context());
        self.prompt = Some(self.parse_prompt());
        let prompt = self.prompt.clone().unwrap();
        self.prefix_user = self.prefix_user.clone().or_else(|| match &*prompt {
            DEFAULT_CHAT_PROMPT_WRAPPER => Some(String::from("HUMAN: ")),
            _ => None
        });
        self.prefix_ai = self.prefix_ai.clone().or_else(|| match &*prompt {
            DEFAULT_CHAT_PROMPT_WRAPPER => Some(String::from("AI: ")),
            _ => None
        });

        let session_dir = {
            let mut dir = config.dir.clone();
            dir.push("sessions");
            dir
        };
        fs::create_dir_all(&session_dir).expect("Config directory could not be created");

        if self.overwrite {
            let path = {
                let mut path = session_dir.clone();
                path.push(name.as_ref().unwrap());
                path
            };
            let file = OpenOptions::new().write(true).truncate(true).open(path);
            if let Ok(mut session_file) = file {
                session_file.write_all(b"").expect("Unable to write to session file");
                session_file.flush().expect("Unable to write to session file");
            }
        }

        let mut current_transcript = String::new();
        let mut session_file: Option<File> = if let Some(name) = name {
            let path = {
                let mut path = session_dir.clone();
                path.push(name);
                path
            };

            match fs::read_to_string(&path) {
                Ok(mut session_config) if !self.override_session_configuration => {
                    let divider_index = session_config.find("<->")
                        .expect("Valid session files have a <-> divider");

                    current_transcript = session_config
                        .split_off(divider_index + 4)
                        .trim()
                        .to_string();
                    session_config.split_off(divider_index);

                    let config: SessionCommand = serde_yaml::from_str(&session_config)
                        .expect("Serializing self to yaml config should work 100% of the time");

                    self.override_config_with_saved_session(config);

                    Some(OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(path)
                        .expect("Unable to open session file")
                    )
                },
                Ok(mut session_config) => {
                    todo!();
                },
                Err(_) => {
                    let config = serde_yaml::to_string(self)
                        .expect("Serializing self to yaml config should work 100% of the time");

                    let mut file = OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(path)
                        .expect("Unable to open session file");

                    if let Err(e) = writeln!(file, "{}<->", &config) {
                        eprintln!("Couldn't write new configuration to file: {}", e);
                    }

                    Some(file)
                }
            }
        } else {
            None
        };

        if self.print_default_prompts {
            print_default_prompts();
            return Ok(vec![]);
        }

        // The commands need to be instantiated before printing the opening prompt because they can
        // print warnings about mismatched options without failing.
        let command = match self.provider {
            Provider::OpenAI => Ok(OpenAISessionCommand::try_from(&*self)?),
            Provider::Cohere => Err(CohereSessionCommand::try_from(&*self)?),
        };

        print_opening_prompt(&self, &current_transcript);

        let mut line = if !self.ai_responds_first {
            let line = self.append_string.clone()
                .or_else(|| read_next_user_line(self.prefix_user.as_ref()));

            match line {
                Some(ref line) => match &self.prefix_user {
                    None => write_next_line(&line, &mut current_transcript, session_file.as_mut()),
                    Some(prefix) => {
                        let combined = format!("{}{}", prefix, line);
                        write_next_line(&combined, &mut current_transcript, session_file.as_mut());
                    },
                },
                None => return Ok(vec![]),
            }
            line
        } else {
            Some(String::new())
        };

        loop {
            let prompt = match (self.no_context.unwrap(), &self.prefix_ai) {
                (true, None) => line.unwrap(),
                (true, Some(prefix)) => format!("{}{}", line.unwrap(), prefix),
                (false, None) => prompt.replace("${TRANSCRIPT}", &current_transcript),
                (false, Some(prefix)) =>
                    prompt.replace("${TRANSCRIPT}", &current_transcript) + &prefix
            };

            let result = match &command {
                Ok(command) => command.run(client, config, &prompt).await?,
                Err(command) => command.run(client, config, &prompt).await?,
            };

            if let Some(count) = self.response_count {
                if count > 1 {
                    return Ok(result);
                }
            }

            let text = result.first().unwrap().trim();
            let written_response = match &self.prefix_ai {
                Some(prefix) => format!("{}{}", prefix, text),
                None => text.to_owned()
            };
            write_next_line(&written_response, &mut current_transcript, session_file.as_mut());

            if !self.quiet {
                println!("{}", written_response);
            }

            if self.append || self.append_string.is_some() {
                return Ok(vec![ text.to_string() ]);
            }

            line = read_next_user_line(self.prefix_user.as_ref());
            match line {
                None => return Ok(vec![]),
                Some(ref line) => match &self.prefix_user {
                    None => write_next_line(&line, &mut current_transcript, session_file.as_mut()),
                    Some(prefix) => {
                        let combined = format!("{}{}", prefix, line);
                        write_next_line(&combined, &mut current_transcript, session_file.as_mut());
                    }
                }
            }
        }
    }

    fn override_config_with_saved_session(&mut self, saved: SessionCommand) {
        self.ai_responds_first = saved.ai_responds_first;
        self.append = saved.append;
        self.append_string = saved.append_string;
        self.model = saved.model;
        self.model_focus = saved.model_focus;
        self.temperature = saved.temperature;
        self.name = saved.name;
        self.overwrite = saved.overwrite;
        self.prompt = saved.prompt;
        self.prompt_path = saved.prompt_path;
        self.no_context = saved.no_context;
        self.quiet = saved.quiet;
        self.prefix_user = saved.prefix_user;
        self.prefix_ai = saved.prefix_ai;
    }

    fn validate(&self) -> Result<(), SessionError> {
        if self.name.is_none() {
            if self.append {
                return Err(SessionError::AppendRequiresSession);
            }

            if self.overwrite {
                return Err(SessionError::OverwriteRequiresSession);
            }

            if self.quiet {
                return Err(SessionError::QuietRequiresSession);
            }
        }

        if self.append_string.is_some() && self.append {
            return Err(SessionError::AppendsClash);
        }

        if self.ai_responds_first && self.append_string.is_some() {
            return Err(SessionError::AppendAndAiRespondsFirstIsNonsensical);
        }

        if let Some(count) = self.response_count {
            if count == 0 {
                return Err(SessionError::ZeroResponseCountIsNonsensical);
            }
        }

        Ok(())
    }

    pub fn parse_no_context(&self) -> bool {
        self.no_context.unwrap_or_else(|| {
            match self.model_focus {
                ModelFocus::Text => false,
                ModelFocus::Code => true,
            }
        })
    }

    pub fn parse_prompt(&self) -> String {
        self.prompt
            .clone()
            .or_else(|| {
                self.prompt_path
                    .clone()
                    .and_then(|path| {
                        std::fs::read_to_string(path).ok()
                    })
            })
            .unwrap_or_else(|| {
                match self.model_focus {
                    ModelFocus::Text => DEFAULT_CHAT_PROMPT_WRAPPER.to_owned(),
                    ModelFocus::Code => DEFAULT_CODE_PROMPT_WRAPPER.to_owned(),
                }
            })
    }
}

#[derive(Copy, Clone, Debug, Default, ValueEnum, Serialize, Deserialize)]
pub enum Provider {
    /// Cohere
    Cohere,

    /// OpenAI
    #[default]
    OpenAI,
}

#[derive(Copy, Clone, Debug, Default, ValueEnum, Serialize, Deserialize)]
pub enum Model {
    /// In the range of 0 - 1 billion parameters. OpenAI's Ada, Cohere's "small" option.
    Tiny,

    /// In the range of 1 - 5 billion parameters. OpenAI's Babbage option.
    Small,

    /// In the range of 5 - 10 billion parameters. OpenAI's Curie, Cohere's "medium" option.
    Medium,

    /// In the range of 10 - 50 billion parameters. Cohere's large option.
    Large,

    /// In the range of 50 - 150 billion paramaters. Cohere's xlarge option.
    XLarge,

    /// Greater than 150 billion paramaters. OpenAI's davinci model.
    #[default]
    XXLarge
}

#[derive(Copy, Clone, Default, Debug, ValueEnum, Serialize, Deserialize)]
pub enum ModelFocus {
    Code,
    #[default]
    Text
}

fn print_opening_prompt(args: &SessionCommand, session_file: &str) {
    if args.append {
        return;
    }

    if args.quiet {
        return;
    }

    if session_file.len() > 0 {
        println!("{}", session_file);
    } else {
        match &args.prompt {
            Some(_) => {
                println!(concat!(
                    "\nHello, I'm an AI using a {} model. ",
                    "The prompt used is:\n\n"),
                    args.model.to_possible_value().unwrap().get_name()
                );
                println!("\nWith prompt:\n {}", args.parse_prompt().replace("${TRANSCRIPT}", ""));
            },
            None => {
                println!(concat!("\n",
                    "Hello, I'm an AI using a {} model. ",
                    "Ask me anything."),
                    args.model.to_possible_value().unwrap().get_name()
                );
            }
        }
    }
}

fn print_default_prompts() {
    println!(concat!(
        "\n",
        "The default prompt for chat models is:\n",
        "----------------------------------------\n",
        "{}\n\n",
        "________________________________________\n\n",
        "And the default for code prompts is:\n",
        "----------------------------------\n\n",
        "{}\n"),
        DEFAULT_CHAT_PROMPT_WRAPPER, DEFAULT_CODE_PROMPT_WRAPPER);
}

fn read_next_user_line(prefix_user: Option<&String>) -> Option<String> {
    let mut rl = rustyline::Editor::<()>::new().expect("Failed to create rusty line editor");
    let default = String::new();
    let prefix = prefix_user.unwrap_or(&default);

    match rl.readline(prefix) {
        Ok(line) => Some(String::from("") + line.trim_end()),
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

const DEFAULT_CODE_PROMPT_WRAPPER: &str = "${TRANSCRIPT}";
const DEFAULT_CHAT_PROMPT_WRAPPER: &str = "
The following is a transcript between a helpful AI assistant and a human. The AI assistant can provide factual information (but only from before mid 2021, when its training data cuts off), ask clarifying questions, and engage in chit chat.

Transcript:

${TRANSCRIPT}

";
