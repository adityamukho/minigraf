use graphql_parser::query;
use graphql_parser::schema;

pub mod error_codes {
    pub const ERROR_SCHEMA_INVALID: i32 = 1;
    pub const ERROR_QUERY_INVALID: i32 = 2;
}

pub fn load_schema(schema: &str) -> schema::Document<String> {
    let schema = schema::parse_schema::<String>(schema).unwrap_or_else(|err| {
        eprintln!("Error parsing schema: {}", err);
        std::process::exit(error_codes::ERROR_SCHEMA_INVALID);
    });
    dbg!("schema: {:#?}", &schema);

    schema
}

pub fn parse_query(query: &str) -> query::Document<String> {
    let query = query::parse_query::<String>(query).unwrap_or_else(|err| {
        eprintln!("Error parsing query: {}", err);
        std::process::exit(error_codes::ERROR_QUERY_INVALID);
    });
    dbg!("query: {:#?}", &query);

    query
}
