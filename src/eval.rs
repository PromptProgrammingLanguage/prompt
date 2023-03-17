use ai::{Config,ChatCommand,ChatResult,CompletionOptions};
use reqwest::Client;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use super::parser;
use super::ast::{Prompt,MatchAction,MatchStatement,Statement};

#[derive(Debug)]
pub struct Evaluator {
    pub client: Client,
    pub config: EvaluatorConfig
}

#[derive(Debug)]
pub struct EvaluatorConfig {
    pub api_key: String,
    pub prompt_path: PathBuf,
}

#[derive(Debug)]
pub struct EvaluateEnvironment {
    pub ai: String,
    pub user: String
}

pub enum EvaluatorError {

}

impl Evaluator {
    pub async fn eval(&self) -> Result<(), EvaluatorError> {
        if !self.config.prompt_path.is_file() {
            panic!("prompt path is not a file");
        }

        let file = fs::read_to_string(&self.config.prompt_path)
            .expect(&format!("Failed to open {}", "fooabr"));

        let folder_name = self.config.prompt_path.file_stem().unwrap().to_str().unwrap().to_string();
        let output_dir = {
            let path = self.config.prompt_path.clone();
            let parent = path.parent().clone().unwrap();

            path.parent().unwrap().join(&folder_name)
        };

        if !output_dir.exists() {
            fs::create_dir(&output_dir).expect("Could not create output dir");
        }

        let program = parser::parse::program(&file)
            .expect("Couldn't parse the prompt program correctly")
            .expect("Couldn't parse the prompt program correctly");

        let config = Config {
            api_key: Some(self.config.api_key.clone()),
            dir: output_dir,
            ..Config::default()
        };

        evaluate_prompt(program.prompts.first().unwrap(), &self.client, &config).await;

        Ok(())
    }
}

async fn evaluate_prompt(prompt: &Prompt, client: &Client, config: &Config) {
    let command = ChatCommand {
        completion: CompletionOptions {
            ai_responds_first: prompt.options.eager.clone(),
            no_context: prompt.options.history.clone().map(|h| !h),
            name: Some(prompt.name.clone()),
            once: Some(true),
            stream: Some(false),
            quiet: Some(true),
            ..CompletionOptions::default()
        },
        system: prompt.options.system.clone()
    };

    let mut result = command.run(client, &config).await.unwrap();

    let env = EvaluateEnvironment {
        ai: result.pop().unwrap().content,
        user: result.pop().unwrap().content,
    };

    for statement in prompt.statements.iter() {
        match statement {
            Statement::MatchStatement(MatchStatement { variable, cases }) => {
                let test = match &*variable.0 {
                    "AI" => env.ai.clone(),
                    "USER" => env.user.clone(),
                    _ => panic!("Unexpected variable")
                };

                for case in cases {
                    if true {
                        evaluate_match_action(&env, &case.action).await
                    }
                }
            },
            _ => todo!()
        }
    }
}

async fn evaluate_match_action(env: &EvaluateEnvironment, action: &MatchAction) {
    match action {
        MatchAction::BashCommand(command) => {
            let output = if cfg!(target_os = "windows") {
                Command::new("cmd")
                        .env("AI", &env.ai)
                        .env("USER", &env.user)
                        .args(["/C", &command.0])
                        .output()
                        .expect("failed to execute process")
            } else {
                Command::new("sh")
                        .env("AI", &env.ai)
                        .env("USER", &env.user)
                        .arg("-c")
                        .arg(&command.0)
                        .output()
                        .expect("failed to execute process")
            };
            // TODO: Handle errors on stderr... somehow
            let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("{}", result);
        },
        _ => todo!()
    }
}
