use prompt::{prompt,PromptArgs};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let path = PathBuf::from("./examples/animal_house.pr");

    let args = PromptArgs {
        path,
        quiet: false,
        watch: None
    };

    prompt(args).await;
}

#[cfg(test)]
mod tests {
    const ANIMAL_HOUSE: &'static str = include_str!("./animal_house.pr");

    use prompt::parser;

    #[test]
    fn example_animal_house_parses_correctly() {
        parser::parse::program(ANIMAL_HOUSE).unwrap();
    }
}
