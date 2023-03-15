use super::ast::*;

peg::parser! {
    grammar parser() for str {
        rule _() = [' ' | '\t' | '\r' | '\n']*

        rule variable_char() -> char
            = ['a'..='z' | 'A'..='Z' | '0'..='9' | '_']

        pub rule variable() -> Variable
            = "$" chars:variable_char()+ { 
                Variable(chars.into_iter().collect::<String>())
            }

        pub rule prompt_call() -> PromptCall
            = call:variable_char()+ awaited:".await"? { 
                PromptCall {
                    call: call.into_iter().collect::<String>(),
                    awaited: awaited.is_some()
                }
            }

        pub rule regex() -> Regex
            = "/" regex_body:$(!"/" [_])* "/" regex_flags:$(['i' | 'm' | 's' | 'x']*) {
                Regex(format!("/{}/{}", regex_body.into_iter().collect::<String>(), regex_flags))
            }

        pub rule bash_command() -> BashCommand
            = "`" bash_command_body:$(!"`" [_])* "`" { 
                BashCommand(bash_command_body.into_iter().collect::<String>())
            }

        pub rule match_statement() -> MatchStatement
            = "match" _ variable:variable() _ "{" _ cases:match_case() ** "," _ "}" _ {
                MatchStatement { variable, cases }
            }

        rule match_case() -> MatchCase
            = _ regex:regex() _ "=>" _ bash_command:bash_command() _ {
                MatchCase { regex, action: MatchAction::BashCommand(bash_command) } 
            }
            / _ regex:regex() _ "=>" _ prompt_call:prompt_call() _ {
                MatchCase { regex, action: MatchAction::PromptCall(prompt_call) } 
            }

        pub rule pipe_statement() -> PipeStatement
            = variable:variable() _ "=>" _ prompt_call:prompt_call() {
                PipeStatement {
                    variable,
                    prompt_call
                }
            }

        pub rule statement() -> Statement
            = s:match_statement() _ { Statement::MatchStatement(s) }
            / s:prompt_call() _ { Statement::PromptCall(s) }
            / s:pipe_statement() _ { Statement::PipeStatement(s) }

        pub rule statements() -> Vec<Statement>
            = _ statements:(statement()) ** _ { statements }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn parse_match_statement_with_no_actions() {
        let match_statement = "match $variable {}";

        assert_eq!(parser::match_statement(match_statement).unwrap(), MatchStatement {
            variable: Variable(String::from("variable")),
            cases: vec![]
        });
    }

    #[test]
    fn parse_match_statement() {
        let match_statement = "match $variable {
            /^yes/i => go_ahead,
            /^no/i => `handle_error`
        }";

        assert_eq!(parser::match_statement(match_statement).unwrap(), MatchStatement {
            variable: Variable(String::from("variable")),
            cases: vec![
                MatchCase {
                    regex: Regex(String::from("/^yes/i")),
                    action: MatchAction::PromptCall(PromptCall {
                        call: String::from("go_ahead"),
                        awaited: false
                    })
                },
                MatchCase {
                    regex: Regex(String::from("/^no/i")),
                    action: MatchAction::BashCommand(BashCommand(String::from("handle_error")))
                },
            ]
        });
    }

    #[test]
    fn parse_prompt_call() {
        let prompt_call = "foo.await";
        assert_eq!(
            parser::prompt_call(prompt_call).unwrap(),
            PromptCall {
                call: String::from("foo"),
                awaited: true
            }
        );

        let prompt_call = "bar";
        assert_eq!(
            parser::prompt_call(prompt_call).unwrap(),
            PromptCall {
                call: String::from("bar"),
                awaited: false
            }
        );

        let prompt_call = "$_invalid";
        assert!(parser::prompt_call(prompt_call).is_err());
    }

    #[test]
    fn parse_pipe_statement() {
        let pipe_statement = "$LINE => foo";
        assert_eq!(
            parser::pipe_statement(pipe_statement).unwrap(),
            PipeStatement {
                variable: Variable(String::from("LINE")),
                prompt_call: PromptCall {
                    call: String::from("foo"),
                    awaited: false
                }
            }
        );
    }

    #[test]
    fn parse_multiple_different_statement() {
        let input = r#"
            match $variable {
                /^yes/i => go_ahead,
                /^no/i => `handle_error`
            }
            foo.await
            $bar => baz
        "#;

        assert_eq!(parser::statements(input).unwrap(), vec![
            Statement::MatchStatement(MatchStatement {
                variable: Variable(String::from("variable")),
                cases: vec![
                    MatchCase {
                        regex: Regex(String::from("/^yes/i")),
                        action: MatchAction::PromptCall(PromptCall {
                            call: String::from("go_ahead"),
                            awaited: false
                        })
                    },
                    MatchCase {
                        regex: Regex(String::from("/^no/i")),
                        action: MatchAction::BashCommand(BashCommand(String::from("handle_error")))
                    },
                ]
            }),
            Statement::PromptCall(PromptCall {
                call: String::from("foo"),
                awaited: true
            }),
            Statement::PipeStatement(PipeStatement {
                variable: Variable(String::from("bar")),
                prompt_call: PromptCall {
                    call: String::from("baz"),
                    awaited: false
                }
            }),
        ]);
    }
}

