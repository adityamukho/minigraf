use std::{env, fs, io, process};
use std::io::Write;

fn main() {
    // Get the schema file from the command line arguments or use the default.
    let default_schema_file = "schema.graphql".to_string();
    let schema_file = &env::args().nth(1).unwrap_or(default_schema_file);
    dbg!("schema_file: {}", schema_file);

    // Read the schema file and parse it into a schema object.
    let schema_file = fs::read_to_string(schema_file).unwrap_or_else(|err| {
        eprintln!("Error reading schema file: {}", err);
        process::exit(error_codes::ERROR_INVALID_SCHEMA_FILE);
    });

    let schema = minigraf::parse_schema(&schema_file).unwrap_or_else(|err| {
        eprintln!("Error parsing schema: {}", err);
        process::exit(error_codes::ERROR_INVALID_SCHEMA);
    });
    dbg!("schema: {:#?}", &schema);

    // Start a Read-Eval-Print-Loop (REPL) for the user to enter queries.
    loop {
        // Prompt the user for a query.
        print!("> ");
        io::stdout().flush().unwrap();

        // Get the query from the user.
        let mut query = String::new();
        io::stdin().read_line(&mut query).unwrap();
        let query = query.trim();

        // Parse the query into a query object.
        let query = minigraf::parse_query(query);
        if query.is_err() {
            eprintln!("Error parsing query: {}", query.unwrap_err());
            continue;
        }
        dbg!("query: {:#?}", &query);
    }
}

mod error_codes {
    pub const ERROR_INVALID_SCHEMA_FILE: i32 = 1;
    pub const ERROR_INVALID_SCHEMA: i32 = 2;
}
