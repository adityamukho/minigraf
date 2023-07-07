use std::io::Write;
use std::{env, fs, io, process};

mod error_codes {
    pub const ERROR_INVALID_SCHEMA_FILE: i32 = 1;
    pub const ERROR_INVALID_SCHEMA: i32 = 2;
    pub const ERROR_BAD_QUERY: i32 = 3;
}

fn error_log<E>(error_code: i32, message: &str, err: E)
where
    E: std::fmt::Display,
{
    eprintln!("ERROR {}: {}: {}", error_code, message, err);
}

fn main() {
    // Get the schema file from the command line arguments or use the default.
    let default_schema_file = "schema.graphql".to_string();
    let schema_file = &env::args().nth(1).unwrap_or(default_schema_file);
    dbg!("schema_file: {}", schema_file);

    // Read the schema file and parse it into a schema object.
    let schema_file = fs::read_to_string(schema_file).unwrap_or_else(|err| {
        error_log(
            error_codes::ERROR_INVALID_SCHEMA_FILE,
            "Couldn't read schema file",
            err,
        );
        process::exit(error_codes::ERROR_INVALID_SCHEMA_FILE);
    });

    let schema = minigraf::parse_schema(&schema_file).unwrap_or_else(|err| {
        error_log(
            error_codes::ERROR_INVALID_SCHEMA,
            "Couldn't parse schema",
            err,
        );
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
            error_log(
                error_codes::ERROR_BAD_QUERY,
                "Couldn't parse query",
                query.err().unwrap(),
            );
            continue;
        }
        dbg!("query: {:#?}", &query);
    }
}
