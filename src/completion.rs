use clap::Args;
use serde::{Serialize,Deserialize};
use serde::de::DeserializeOwned;
use std::fs::{self,File,OpenOptions};
use std::io::{self,Write};
use crate::Config;

#[derive(Args, Clone, Default, Debug, Serialize, Deserialize)]
pub struct CompletionOptions {
    /// Allow the AI to generate a response to the prompt before user input
    #[arg(long)]
    pub ai_responds_first: Option<bool>,

    /// Append a string to an existing session and get only the latest response.
    #[arg(long)]
    pub append: Option<String>,

    /// Temperature of the model, the allowed range of this value is different across providers,
    /// for OpenAI it's 0 - 2, and Cohere uses a 0 - 5 scale.
    #[arg(long, short)]
    pub temperature: Option<f32>,

    /// Saves your conversation context using the session name
    #[arg(short, long)]
    pub name: Option<String>,

    /// Overwrite the existing session if it already exists
    #[arg(long)]
    pub overwrite: Option<bool>,

    /// Only write output the session file
    #[arg(long)]
    pub quiet: Option<bool>,

    /// Prefix ai responses with the supplied string. This can be used for labels if your prompt has
    /// a conversational style. Defaults to "AI"
    #[arg(long)]
    pub prefix_ai: Option<String>,

    /// Prefix input with the supplied string. This can be used for labels if your prompt has a
    /// conversational style. Defaults to "USER:"
    #[arg(long)]
    pub prefix_user: Option<String>,

    /// Stream the output to the terminal
    #[arg(long)]
    pub stream: Option<bool>,
}

impl CompletionOptions {
    pub fn merge(&self, merged: &CompletionOptions) -> Self {
        let original = self.clone();
        let merged = merged.clone();

        CompletionOptions {
            ai_responds_first: original.ai_responds_first.or(merged.ai_responds_first),
            append: original.append.or(merged.append),
            temperature: original.temperature.or(merged.temperature),
            name: original.name.or(merged.name),
            overwrite: original.overwrite.or(merged.overwrite),
            quiet: original.quiet.or(merged.quiet),
            prefix_ai: original.prefix_ai.or(merged.prefix_ai),
            prefix_user: original.prefix_user.or(merged.prefix_user),
            stream: original.stream.or(merged.stream),
        }
    }

    pub fn load_session_file<T>(&self, config: &Config) -> CompletionFile<T>
    where
        T: Clone + Default + DeserializeOwned + Serialize
    {
        let session_dir = {
            let mut dir = config.dir.clone();
            dir.push("sessions");
            dir
        };
        fs::create_dir_all(&session_dir).expect("Config directory could not be created");

        if self.overwrite.unwrap_or(false) {
            let path = {
                let mut path = session_dir.clone();
                path.push(self.name.as_ref().unwrap());
                path
            };
            let file = OpenOptions::new().write(true).truncate(true).open(path);
            if let Ok(mut session_file) = file {
                session_file.write_all(b"").expect("Unable to write to session file");
                session_file.flush().expect("Unable to write to session file");
            }
        }

        let file = self.name.clone().map(|name| {
            let path = {
                let mut path = session_dir.clone();
                path.push(name);
                path
            };

            let mut transcript = String::new();
            let mut overrides = T::default();
            let file = match fs::read_to_string(&path) {
                Ok(mut session_config) => {
                    let divider_index = session_config.find("<->")
                        .expect("Valid session files have a <-> divider");

                    transcript = session_config
                        .split_off(divider_index + 4)
                        .trim()
                        .to_string();
                    session_config.truncate(divider_index);
                    overrides = serde_yaml::from_str(&session_config)
                        .expect("Serializing self to yaml config should work 100% of the time");

                    OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(path)
                        .expect("Unable to open session file")
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

                    file
                }
            };

            CompletionFile {
                file: Some(file),
                overrides,
                transcript
            }
        });

        file.unwrap_or_default()
    }
}

#[derive(Default)]
pub struct CompletionFile<T: Clone + Default + DeserializeOwned + Serialize = CompletionOptions> {
    pub file: Option<File>,
    pub overrides: T,
    pub transcript: String
}

impl CompletionFile {
    pub fn write_words(&mut self, words: String) -> io::Result<String> {
        match &mut self.file {
            Some(file) => match write!(file, "{}", words) {
                Ok(()) => { self.transcript += &words; Ok(words) },
                Err(e) => Err(e)
            },
            None => { self.transcript += &words; Ok(words) }
        }
    }

    pub fn write(&mut self, line: String) -> io::Result<String> {
        match &mut self.file {
            Some(file) => match writeln!(file, "{}", line) {
                Ok(()) => {
                    self.transcript += &line;
                    self.transcript += "\n";
                    Ok(line)
                },
                Err(e) => Err(e)
            },
            None => {
                self.transcript += &line;
                self.transcript += "\n";
                Ok(line)
            }
        }
    }

    pub fn read(&mut self, append: Option<&str>, prefix_user: Option<&str>) -> Option<String> {
        let line = append
            .map(|s| s.to_string())
            .or_else(|| read_next_user_line(prefix_user))
            .map(|s| s.trim().to_string());

        line.and_then(|line| match &prefix_user {
            None => self.write(line).ok(),
            Some(prefix) => self.write(format!("{}: {}", prefix, line)).ok(),
        })
    }
}

fn read_next_user_line(prefix_user: Option<&str>) -> Option<String> {
    let mut rl = rustyline::Editor::<()>::new().expect("Failed to create rusty line editor");
    let prefix = match prefix_user {
        Some(user) => format!("{}: ", user),
        None => String::new(),
    };

    match rl.readline(&prefix) {
        Ok(line) => Some(String::from("") + line.trim_end()),
        Err(_) => None
    }
}