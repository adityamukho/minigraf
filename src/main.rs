fn main() {
    // Get the schema file from the command line arguments or use the default.
    let default_schema_file = "schema.graphql".to_string();
    let schema_file = &std::env::args().nth(1).unwrap_or(default_schema_file);
    println!("schema_file: {}", schema_file);

    // Read the schema file and parse it into a schema object.
    let result = std::fs::read_to_string(schema_file);
    if result.is_err() {
        println!("Error reading schema file: {}", result.err().unwrap());
        std::process::exit(error_codes::ERROR_SCHEMA_FILE_INVALID);
    }

    let schema_file = result.unwrap();
    let result = minigraf::parse_schema(&schema_file);
    if result.is_err() {
        println!("Error parsing schema file: {}", result.err().unwrap());
        std::process::exit(minigraf::error_codes::ERROR_SCHEMA_INVALID);
    }

    let schema = result.unwrap();
    println!("schema: {:#?}", schema);
}

mod error_codes {
    pub const ERROR_SCHEMA_FILE_INVALID: i32 = 0x100;
}
