use serde::Deserialize;
use regex::Regex;

#[derive(Debug, PartialEq)]
pub struct Program {
    pub prompts: Vec<Prompt>
}

#[derive(Debug, PartialEq)]
pub struct Prompt {
    pub name: String,
    pub options: PromptOptions,
    pub statements: Vec<Statement>
}

#[derive(Debug, Default, PartialEq, Deserialize)]
pub struct PromptOptions {
    pub eager: Option<bool>,
    pub history: Option<bool>,
    pub system: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Statement {
    MatchStatement(MatchStatement),
    PromptCall(PromptCall),
    PipeStatement(PipeStatement),
}

#[derive(Debug, PartialEq)]
pub struct MatchStatement {
    pub variable: Variable,
    pub cases: Vec<MatchCase>,
}

#[derive(Debug)]
pub struct MatchCase {
    pub regex: Regex,
    pub action: MatchAction,
}

impl PartialEq for MatchCase {
    fn eq(&self, other: &MatchCase) -> bool {
        return &self.action == &other.action && self.regex.as_str() == other.regex.as_str()
    }
}

#[derive(Debug, PartialEq)]
pub enum MatchAction {
    Command(Command),
    PromptCall(PromptCall)
}

#[derive(Debug, PartialEq)]
pub struct PipeStatement {
    pub variable: Variable,
    pub prompt_call: PromptCall,
}

#[derive(Debug, PartialEq)]
pub struct PromptCall {
    pub call: String,
    pub awaited: bool
}

#[derive(Debug, PartialEq)]
pub struct Command(pub String);

#[derive(Debug, PartialEq)]
pub struct Variable(pub String);
