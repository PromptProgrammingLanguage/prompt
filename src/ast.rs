use serde::Deserialize;
use regex::Regex;

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub prompts: Vec<Prompt>
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub is_main: bool,
    pub name: String,
    pub options: PromptOptions,
    pub statements: Vec<Statement>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PromptOptions {
    pub direction: Option<String>,
    pub eager: Option<bool>,
    pub history: Option<bool>,
    pub system: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    MatchStatement(MatchStatement),
    PromptCall(PromptCall),
    PipeStatement(PipeStatement),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchStatement {
    pub variable: Variable,
    pub cases: Vec<MatchCase>,
}

#[derive(Clone, Debug)]
pub struct MatchCase {
    pub regex: Regex,
    pub action: MatchAction,
}

impl PartialEq for MatchCase {
    fn eq(&self, other: &MatchCase) -> bool {
        return &self.action == &other.action && self.regex.as_str() == other.regex.as_str()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MatchAction {
    Command(Command),
    PromptCall(PromptCall)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipeStatement {
    pub variable: Variable,
    pub prompt_call: PromptCall,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PromptCall {
    pub name: String,
    pub awaited: bool
}

#[derive(Clone, Debug, PartialEq)]
pub struct Command(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct Variable(pub String);
