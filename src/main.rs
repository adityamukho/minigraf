use std::io;
use std::io::Write;

use graphql_parser::query::parse_query;

fn main() {
    loop {
        // Prompt user
        print!("> ");
        io::stdout().flush().unwrap();

        // Read input from stdin
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();

        // Parse input
        let ast = parse_query::<&str>(input);

        println!("{:#?}", ast);
    }
}

// fn run() {}
