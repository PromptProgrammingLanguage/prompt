use ai::{Config,ChatCommand,ChatResult,CompletionOptions};
use reqwest::Client;
use std::fs;
use std::path::PathBuf;
use std::process;
use regex::{Captures,CaptureNames};
use super::parser;
use super::ast::*;

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

#[derive(Debug)]
pub enum EvaluatorError {

}

pub type EvaluatorResult = Result<String, EvaluatorError>;

impl Evaluator {
    pub async fn eval(&self) -> EvaluatorResult {
        if !self.config.prompt_path.is_file() {
            panic!("prompt path is not a file");
        }

        let file = fs::read_to_string(&self.config.prompt_path)
            .expect(&format!("Failed to open {}", "fooabr"));

        let program = parser::parse::program(&file)
            .expect("Couldn't parse the prompt program correctly")
            .expect("Couldn't parse the prompt program correctly");

        let config = Config {
            api_key: Some(self.config.api_key.clone()),
            dir: self.config.prompt_path.parent().unwrap().to_path_buf(),
            ..Config::default()
        };

        let result = evaluate_prompt(program.prompts.first().unwrap(), &self.client, &config).await;
        match &result {
            Ok(r) => println!("{}", r),
            Err(e) => eprintln!("{:#?}", e)
        }

        result
    }
}

async fn evaluate_prompt(prompt: &Prompt, client: &Client, config: &Config) -> EvaluatorResult {
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
            Statement::MatchStatement(match_statement) => {
                return evaluate_match_statement(&env, match_statement).await
            },
            _ => todo!()
        }
    }
    Ok(String::new())
}

async fn evaluate_match_statement(
    env: &EvaluateEnvironment,
    statement: &MatchStatement) -> EvaluatorResult
{
    let MatchStatement { variable, cases } = statement;
    let test = match &*variable.0 {
        "AI" => env.ai.clone(),
        "USER" => env.user.clone(),
        _ => panic!("Unexpected variable")
    };

    for case in cases {
        if let Some(captures) = case.regex.captures(&test) {
            let names = &mut case.regex.capture_names();

            return evaluate_match_action(&env, &case.action, &captures, names).await;
        }
    }

    Ok(String::new())
}

async fn evaluate_match_action(
    env: &EvaluateEnvironment,
    action: &MatchAction,
    captures: &Captures<'_>,
    capture_names: &mut CaptureNames<'_>) -> EvaluatorResult
{
    match action {
        MatchAction::Command(ref command) => {
            evaluate_match_action_command(env, command, captures, capture_names)
        },
        _ => todo!()
    }
}

fn evaluate_match_action_command(
    env: &EvaluateEnvironment,
    command: &Command,
    captures: &Captures<'_>,
    capture_names: &mut CaptureNames<'_>) -> EvaluatorResult
{
    let output = if cfg!(target_os = "windows") {
        process::Command::new("cmd")
                .env("AI", &env.ai)
                .env("USER", &env.user)
                .args(["/C", &command.0])
                .output()
                .expect("failed to execute process")
    } else {
        let mut process = process::Command::new("sh");
        process.env("AI", &env.ai);
        process.env("USER", &env.user);

        let mut i = 0;
        for name in capture_names {
            if let Some(name) = name {
                process.env(name, &captures[name]);
            }
            process.env(&format!("{i}"), &captures[i]);
            i += 1;
        }

        process.arg("-c");
        process.arg(&command.0);
        process.output().expect("failed to execute process")
    };
    // TODO: Handle errors on stderr... somehow
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    #[tokio::test]
    async fn evaluate_match_statement_() {
        let env = &EvaluateEnvironment {
            user: "".into(),
            ai: "Yes. Something else".into()
        };
        let statement = &MatchStatement {
            variable: Variable("AI".into()),
            cases: vec![
                MatchCase {
                    regex: Regex::new("(?i:yes[^a-z]*(?P<FOOBAR>.+))").unwrap(),
                    action: MatchAction::Command(Command("echo $FOOBAR".into()))
                }
            ]
        };
        assert_eq!(evaluate_match_statement(env, statement).await.unwrap(), String::from("Something else"));
    }
}
