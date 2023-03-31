use ai::{Config,ChatCommand,ChatRole,CompletionOptions};
use reqwest::Client;
use std::path::PathBuf;
use std::process;
use tokio::task::JoinError;
use regex::{Captures,CaptureNames};
use super::ast::*;
use futures::future::join_all;
use futures::Future;
use std::pin::Pin;

#[derive(Clone, Debug)]
pub struct Evaluate {
    pub client: Client,
    pub config: EvaluateConfig,
    pub program: Program
}

#[derive(Debug, Clone)]
pub struct EvaluateConfig {
    pub api_key: String,
    pub prompt_path: PathBuf,
    pub prompt_dir: PathBuf,
    pub quiet: bool
}

#[derive(Debug, Clone, Default)]
pub struct EvaluateState {
    pub current_prompt_name: String,
    pub vars: EvaluateVars,
}

#[derive(Debug, Default, Clone)]
pub struct EvaluateVars {
    pub ai: String,
    pub user: String
}

#[derive(Debug)]
pub enum EvaluateError {
    Command(String),
    MissingPrompt(String),
    UndeclaredVariable(String),
    JoinError(JoinError),
    CommandExited
}

impl From<JoinError> for EvaluateError {
    fn from(e: JoinError) -> Self { EvaluateError::JoinError(e) }
}

impl Evaluate {
    pub fn new(client: Client, program: Program, config: EvaluateConfig) -> Self {
        Self { client, config, program }
    }

    pub async fn eval(&self) -> Result<(), EvaluateError> {
        let evaluate = &Evaluate {
            client: self.client.clone(),
            config: self.config.clone(),
            program: self.program.clone()
        };

        let main = evaluate.program.prompts.iter().find(|prompt| prompt.is_main).unwrap();
        let command = &ChatCommand {
            completion: CompletionOptions {
                ai_responds_first: main.options.eager.clone(),
                no_context: main.options.history.clone().map(|h| !h),
                name: Some(main.name.clone()),
                once: Some(true),
                prefix_ai: Some(main.name.clone()),
                stream: Some(false),
                quiet: Some(true),
                ..CompletionOptions::default()
            },
            system: main.options.description.clone(),
            direction: main.options.direction.clone()
        };

        evaluate_prompt(evaluate, main, command).await
    }
}

async fn evaluate_prompt(
    evaluator: &Evaluate,
    prompt: &Prompt,
    command: &ChatCommand) -> Result<(), EvaluateError>
{
    let Evaluate { client, config, .. } = evaluator;

    let config = Config {
        dir: config.prompt_dir.clone(),
        ..Config::default()
    };

    let result = command.run(client, &config).await.unwrap();

    if result.len() == 0 {
        return Err(EvaluateError::CommandExited);
    }

    let state = EvaluateState {
        current_prompt_name: prompt.name.clone(),
        vars: EvaluateVars {
            ai: result.iter().rev()
                .find(|message| message.role == ChatRole::Ai)
                .map(|message| message.content.split_once(':')
                    .map(|(_, content)| content.trim_start().to_string())
                    .unwrap_or_else(|| message.content.to_string())
                )
                .unwrap(),
            user: result.iter().rev()
                .find(|message| message.role == ChatRole::User)
                .map(|message| message.content.split_once(':')
                    .map(|(_, content)| content.trim_start().to_string())
                    .unwrap_or_else(|| message.content.to_string())
                )
                .unwrap_or_default(),
        }
    };

    for statement in prompt.statements.iter() {
        match statement {
            Statement::MatchStatement(match_statement) => {
                let _ = evaluate_match_statement(evaluator, &state, match_statement).await;
            },
            Statement::PipeStatement(pipe_statement) => {
                let _ = evaluate_pipe_statement(evaluator, &state, pipe_statement, None, None)
                    .await;
            },
            Statement::Command(command) => {
                let result = evaluate_command(evaluator, &state, &command, None, None)?;

                if !evaluator.config.quiet {
                    println!("{result}");
                }
            }
        }
    }

    Ok(())
}

async fn evaluate_match_statement(
    evaluator: &Evaluate,
    state: &EvaluateState,
    statement: &MatchStatement) -> Result<(), EvaluateError>
{
    let MatchStatement { variable, cases } = statement;
    let test = match &*variable.0 {
        "AI" => state.vars.ai.clone(),
        "USER" => state.vars.user.clone(),
        _ => panic!("Unexpected variable")
    };

    for case in cases {
        if let Some(captures) = case.regex.captures(&test) {
            let names = &mut case.regex.capture_names();

            return evaluate_match_action(evaluator, state, &case.action, &captures, names).await;
        }
    }

    Ok(())
}

async fn evaluate_match_action(
    evaluator: &Evaluate,
    state: &EvaluateState,
    action: &MatchAction,
    captures: &Captures<'_>,
    capture_names: &mut CaptureNames<'_>) -> Result<(), EvaluateError>
{
    match action {
        MatchAction::Pipe(ref pipe) => {
            evaluate_pipe_statement(evaluator, state, pipe, Some(captures), Some(capture_names))
                .await?;
        },
        MatchAction::Command(ref command) => {
            let result = evaluate_command(
                evaluator, state, command, Some(captures), Some(capture_names))?;

            if !evaluator.config.quiet {
                println!("{result}");
            }
        },
        MatchAction::PromptCall(ref call) => {
            evaluate_prompt_call(evaluator, &state, &call, &captures[1])
                .await
                .into_iter()
                .collect::<Result<Vec<Result<(), EvaluateError>>, JoinError>>()?
                .into_iter()
                .collect::<Result<Vec<()>, EvaluateError>>()?;
        }
    }

    Ok(())
}

async fn evaluate_pipe_statement(
    evaluator: &Evaluate,
    state: &EvaluateState,
    statement: &PipeStatement,
    captures: Option<&Captures<'_>>,
    capture_names: Option<&mut CaptureNames<'_>>) -> Result<(), EvaluateError>
{
    let append = match &statement.subject {
        PipeSubject::Command(command) => {
            evaluate_command(evaluator, state, command, captures, capture_names)?
        },
        PipeSubject::Variable(variable) =>  match &*variable.0 {
            "AI" => state.vars.ai.to_string(),
            "USER" => state.vars.user.to_string(),
            _ => return Err(EvaluateError::UndeclaredVariable(variable.0.clone()))
        }
    };

    evaluate_prompt_call(evaluator, &state, &statement.call, &append)
        .await
        .into_iter()
        .collect::<Result<Vec<Result<(), EvaluateError>>, JoinError>>()?
        .into_iter()
        .collect::<Result<Vec<()>, EvaluateError>>()?;

    Ok(())
}

fn evaluate_prompt_call(
    evaluator: &Evaluate,
    state: &EvaluateState,
    call: &PromptCall,
    append: &str) -> Pin<Box<dyn Future<Output = Vec<Result<Result<(), EvaluateError>, JoinError>>> + Send + 'static>>
{
    let mut handles = vec![];

    for name in call.names.iter() {
        let evaluate = evaluator.clone();
        let prompt = evaluate.program.prompts.iter()
            .find(|p| &p.name == name)
            .ok_or(EvaluateError::MissingPrompt(name.clone().into()))
            .unwrap()
            .clone();
        let append_str = Some(String::from(append));
        let prefix_user = Some(state.current_prompt_name.clone());

        handles.push(tokio::spawn(async move {
            let options = prompt.options.clone();
            let command = ChatCommand {
                completion: CompletionOptions {
                    ai_responds_first: Some(false),
                    append: append_str,
                    no_context: options.history.map(|h| !h),
                    name: Some(prompt.name.clone()),
                    once: Some(true),
                    prefix_ai: Some(prompt.name.clone()),
                    prefix_user,
                    stream: Some(false),
                    quiet: Some(true),
                    ..CompletionOptions::default()
                },
                system: options.description,
                direction: options.direction
            };
            evaluate_prompt(&evaluate, &prompt, &command).await
        }));
    }

    Box::pin(join_all(handles))
}

fn evaluate_command(
    env: &Evaluate,
    state: &EvaluateState,
    command: &Command,
    captures: Option<&Captures<'_>>,
    capture_names: Option<&mut CaptureNames<'_>>) -> Result<String, EvaluateError>
{
    let mut process = process::Command::new(if cfg!(target_os = "windows") {
        "cmd"
    } else {
        "sh"
    });

    process.env("AI", &state.vars.ai);
    process.env("USER", &state.vars.user);
    process.current_dir(env.config.prompt_dir.clone());

    if cfg!(target_os = "windows") {
        process.args(["/C", &command.0]);
    } else {
        process.arg("-c");
        process.arg(&command.0);
    }

    match (capture_names, captures) {
        (Some(capture_names), Some(captures)) => {
            let mut i = 0;
            for name in capture_names {
                if let Some(name) = name {
                    process.env(name, &captures[name]);
                }
                process.arg(captures[i].to_string());
                i += 1;
            }
        },
        _ => {}
    }

    let output = process.output().expect("failed to execute process");

    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if err.len() > 0 {
        Err(EvaluateError::Command(err))
    } else {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    #[tokio::test]
    #[ignore]
    async fn evaluate_match_statement_with_named_group() {
        let env = &mock_evaluator();
        let state = &EvaluateState {
            current_prompt_name: String::new(),
            vars: EvaluateVars {
                user: "".into(),
                ai: "Yes. Something else".into()
            }
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

        /*
        assert_eq!(String::from("Something else"), evaluate_match_statement(env, state, statement)
            .await
            .unwrap());
        */
    }

    #[tokio::test]
    #[ignore]
    async fn evaluate_match_statement_with_position_group() {
        let env = &mock_evaluator();
        let state = &EvaluateState {
            current_prompt_name: String::new(),
            vars: EvaluateVars {
                user: "".into(),
                ai: "Yes. Something else".into()
            }
        };
        let statement = &MatchStatement {
            variable: Variable("AI".into()),
            cases: vec![
                MatchCase {
                    regex: Regex::new("((?i)yes[^a-z]*(.+))").unwrap(),
                    action: MatchAction::Command(Command("echo $M2".into()))
                }
            ]
        };
        /*
        assert_eq!(String::from("Something else"), evaluate_match_statement(env, state, statement)
            .await
            .unwrap());
        */
    }

    fn mock_evaluator() -> Evaluate {
        Evaluate {
            client: reqwest::ClientBuilder::new().build().expect("Client"),
            config: EvaluateConfig {
                api_key: String::new(),
                prompt_path: PathBuf::new(),
                prompt_dir: std::env::current_dir().unwrap(),
                quiet: false
            },
            program: Program {
                prompts: vec![]
            }
        }
    }
}
