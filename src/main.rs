use std::{env, fs, process};

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

    minigraf::load_schema(&schema_file);
}

mod error_codes {
    pub const ERROR_INVALID_SCHEMA_FILE: i32 = -1;
}
