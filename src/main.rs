use std::io::Write;
use std::{env, fs, io, process};

use minigraf::{parse_query, parse_schema};
use server::error_codes::ErrorCode;
use server::logger;

mod server;

fn main() {
    // Get the schema file from the command line arguments or use the default.
    let default_schema_file = "schema.graphql".to_string();
    let schema_file = &env::args().nth(1).unwrap_or(default_schema_file);
    logger::debug_log("schema_file", schema_file);

    // Read the schema file and parse it into a schema object.
    let schema_file = fs::read_to_string(schema_file).unwrap_or_else(|err| {
        logger::error_log(
            ErrorCode::InvalidSchemaFile,
            "Couldn't read schema file",
            err,
        );
        process::exit(ErrorCode::InvalidSchemaFile as i32);
    });

    let schema = parse_schema::<String>(&schema_file).unwrap_or_else(|err| {
        logger::error_log(ErrorCode::InvalidSchema, "Couldn't parse schema", err);
        process::exit(ErrorCode::InvalidSchema as i32);
    });
    logger::debug_log("schema", &schema);

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
        let query = parse_query::<String>(query);
        if query.is_err() {
            logger::error_log(
                ErrorCode::BadQuery,
                "Couldn't parse query",
                query.err().unwrap(),
            );
            continue;
        }
        let query = query.unwrap();
        logger::debug_log("query", &query);
    }
}
