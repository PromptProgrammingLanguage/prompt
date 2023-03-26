use super::ast::*;
use regex::Regex;

peg::parser! {
    pub grammar parse() for str {
        rule _() = quiet!{[' ' | '\t' | '\r' | '\n']*}

        rule variable_char() -> String
            = s:$(['a'..='z' | 'A'..='Z' | '0'..='9' | '_']+) { s.to_string() }

        pub rule variable() -> Variable
            = "$" var:variable_char() {
                Variable(var)
            }

        pub rule regex() -> Regex
            = quiet!{ re:regex_nested() {
                Regex::new(&re).unwrap()
            }}
            / expected!("Valid regular Expression")

        rule regex_nested() -> String
            = "(" b:$([^'('|')']*) n:regex_nested() a:$([^'('|')']*) ")" {
                format!("({b}{n}{a})")
            }
            / "(" c:$([^')']*) ")" { format!("({c})") }

        pub rule command() -> Command
            = "`" command_body:$([^'`']*) "`" {
                Command(command_body.to_string())
            }

        pub rule match_statement() -> MatchStatement
            = "match" _ variable:variable() _ "{" cases:match_cases() "}" _ {
                MatchStatement { variable, cases }
            }

        rule match_cases() -> Vec<MatchCase>
            = _ cases:match_case() ** "," _  { cases }

        rule match_case() -> MatchCase
            = _ regex:regex() _ "=>" _ pipe:pipe_statement() _ {
                MatchCase { regex, action: MatchAction::Pipe(pipe) }
            }
            / _ regex:regex() _ "=>" _ command:command() _ {
                MatchCase { regex, action: MatchAction::Command(command) }
            }
            / _ regex:regex() _ "=>" _ prompt_call:prompt_call() _ {
                MatchCase { regex, action: MatchAction::PromptCall(prompt_call) } 
            }

        pub rule pipe_statement() -> PipeStatement
            = subject:command() _ "->" _ call:prompt_call() {
                PipeStatement { call, subject: PipeSubject::Command(subject) }
            }
            / subject:variable() _ "->" _ call:prompt_call() {
                PipeStatement { call, subject: PipeSubject::Variable(subject) }
            }

        pub rule prompt_name() -> String
            = _ name:variable_char()+ _ { name.into_iter().collect::<String>() }

        pub rule prompt_call() -> PromptCall
            = names:prompt_name() ++ "," {
                PromptCall { names }
            }

        pub rule prompt() -> Result<Prompt, serde_yaml::Error>
            = _ name:prompt_name() _ yaml:$([^'{']*) _ "{" _ statements:statements() _ "}" _ {
                let mut indent = None;
                let yaml = yaml
                    .to_string()
                    .lines()
                    .map(|line| {
                        if indent.is_none() {
                            indent = line.chars().enumerate().find_map(|(i, c)| if c != ' ' {
                                Some(i)
                            } else {
                                None
                            });

                            if indent == Some(0) {
                                indent = None;
                            }
                        }
                        let strip = indent.unwrap_or(0);
                        if line.len() > strip {
                            format!("{}\n", line[strip..].to_string())
                        } else {
                            format!("{}\n", line)
                        }
                    })
                    .collect::<String>();
                    
                let options = match yaml.len() {
                    0 => PromptOptions::default(),
                    _ => serde_yaml::from_str(&yaml)?
                };

                Ok(Prompt { name, options, statements, is_main: false })
            }

        pub rule statement() -> Statement
            = s:match_statement() _ { Statement::MatchStatement(s) }
            / s:prompt_call() _ { Statement::PromptCall(s) }
            / s:pipe_statement() _ { Statement::PipeStatement(s) }

        pub rule statements() -> Vec<Statement>
            = _ statements:(statement()) ** _ { statements }

        pub rule program() -> Result<Program, serde_yaml::Error>
            = _ prompts:prompt()* _ {
                let mut prompts = prompts
                    .into_iter()
                    .collect::<Result<Vec<_>, serde_yaml::Error>>()?;

                if let Some(mut prompt) = prompts.first_mut() {
                    prompt.is_main = true;
                }

                Ok(Program { prompts })
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn parse_program() {
        let program = r#"
            bob {}
            alice {}
        "#;

        assert_eq!(parse::program(program).unwrap().unwrap(), Program {
            prompts: vec![
                Prompt {
                    is_main: true,
                    name: "bob".into(),
                    options: PromptOptions::default(),
                    statements: vec![]
                },
                Prompt {
                    is_main: false,
                    name: "alice".into(),
                    options: PromptOptions::default(),
                    statements: vec![]
                },
            ]
        });
    }


    #[test]
    fn parse_match_statement_with_no_actions() {
        let match_statement = "match $variable {}";

        assert_eq!(parse::match_statement(match_statement).unwrap(), MatchStatement {
            variable: Variable(String::from("variable")),
            cases: vec![]
        });
    }

    #[test]
    fn parse_match_statement() {
        let match_statement = "match $variable {
            (?i:^yes) => go_ahead,
            (?i:^no) => `handle_error`
        }";

        assert_eq!(parse::match_statement(match_statement).unwrap(), MatchStatement {
            variable: Variable(String::from("variable")),
            cases: vec![
                MatchCase {
                    regex: Regex::new("(?i:^yes)").unwrap(),
                    action: MatchAction::PromptCall(PromptCall {
                        names: vec![ String::from("go_ahead") ]
                    })
                },
                MatchCase {
                    regex: Regex::new("(?i:^no)").unwrap(),
                    action: MatchAction::Command(Command(String::from("handle_error")))
                },
            ]
        });
    }

    #[test]
    fn parse_regex() {
        assert_eq!(
            parse::regex("(^foo)").unwrap().as_str(),
            Regex::new("(^foo)").unwrap().as_str()
        );

        assert_eq!(
            parse::regex("((?i)^foo)").unwrap().as_str(),
            Regex::new("((?i)^foo)").unwrap().as_str()
        );

        assert_eq!(
            parse::regex("((?i):^yes)").unwrap().as_str(),
            Regex::new("((?i):^yes)").unwrap().as_str()
        );

        assert_eq!(
            parse::regex("(?i:^yes)").unwrap().as_str(),
            Regex::new("(?i:^yes)").unwrap().as_str()
        );
    }

    #[test]
    fn parse_prompt_call() {
        assert_eq!(
            parse::prompt_call("bar").unwrap(),
            PromptCall {
                names: vec![ String::from("bar") ]
            }
        );

        assert_eq!(
            parse::prompt_call("bar, boo").unwrap(),
            PromptCall {
                names: vec![ String::from("bar"), String::from("boo") ]
            }
        );

        let prompt_call = "$_invalid";
        assert!(parse::prompt_call(prompt_call).is_err());
    }

    #[test]
    fn parse_prompt() {
        let prompt = r#"
            table
                history: false
                description: "Answer this question with a yes or no answer. Is this input valid JSON that can be used with NodeJS's console.table method cleanly?"
            {
                match $AI {}
            }
        "#;

        assert_eq!(parse::prompt(prompt).unwrap().unwrap(), Prompt {
            is_main: false,
            name: "table".into(),
            options: PromptOptions {
                direction: None,
                eager: None,
                history: Some(false),
                description: Some(
                    "Answer this question with a yes or no answer. Is this input valid JSON \
                    that can be used with NodeJS's console.table method cleanly?".into()
                )
            },
            statements: vec![
                Statement::MatchStatement(MatchStatement {
                    variable: Variable(String::from("AI")),
                    cases: vec![]
                })
            ]
        });
    }

    #[test]
    fn parse_pipe_statement_with_variable_subject() {
        let pipe_statement = "$LINE -> foo, bar";
        assert_eq!(
            parse::pipe_statement(pipe_statement).unwrap(),
            PipeStatement {
                subject: PipeSubject::Variable(Variable(String::from("LINE"))),
                call: PromptCall {
                    names: vec![
                        String::from("foo"),
                        String::from("bar"),
                    ]
                }
            }
        );
    }

    #[test]
    fn parse_pipe_statement_with_command() {
        let pipe_statement = "`echo $AI` -> foo";
        assert_eq!(
            parse::pipe_statement(pipe_statement).unwrap(),
            PipeStatement {
                subject: PipeSubject::Command(Command(String::from("echo $AI"))),
                call: PromptCall {
                    names: vec![ String::from("foo") ]
                }
            }
        );
    }

    #[test]
    fn parse_multiple_different_statement() {
        let input = r#"
            match $variable {
                (?i:yes) => go_ahead,
                (?i:no) => `handle_error`
            }
            foo
            $bar -> baz
        "#;

        assert_eq!(parse::statements(input).unwrap(), vec![
            Statement::MatchStatement(MatchStatement {
                variable: Variable(String::from("variable")),
                cases: vec![
                    MatchCase {
                        regex: Regex::new("(?i:yes)").unwrap(),
                        action: MatchAction::PromptCall(PromptCall {
                            names: vec![ String::from("go_ahead") ]
                        })
                    },
                    MatchCase {
                        regex: Regex::new("(?i:no)").unwrap(),
                        action: MatchAction::Command(Command(String::from("handle_error")))
                    },
                ]
            }),
            Statement::PromptCall(PromptCall {
                names: vec![ String::from("foo") ],
            }),
            Statement::PipeStatement(PipeStatement {
                call: PromptCall {
                    names: vec![ String::from("baz") ]
                },
                subject: PipeSubject::Variable(Variable(String::from("bar")))
            }),
        ]);
    }
}

