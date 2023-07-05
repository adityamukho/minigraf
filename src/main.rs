use graphql_parser::parse_schema;

fn main() {
    // Get the schema file from the command line arguments or use the default.
    let default_schema_file = "schema.graphql".to_string();
    let schema_file = &std::env::args().nth(1).unwrap_or(default_schema_file);

    println!("schema_file: {}", schema_file);

    // Read the schema file and parse it into a schema object.
    let schema_file = std::fs::read_to_string(schema_file).unwrap();

    let ast = parse_schema::<String>(&schema_file).unwrap().to_owned();

    println!("{:#?}", ast);
}
