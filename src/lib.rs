use graphql_parser::schema::{Document, ParseError};

pub mod error_codes {
    pub const ERROR_SCHEMA_INVALID: i32 = 1;
}

pub fn parse_schema(schema: &str) -> Result<Document<String>, ParseError> {
    graphql_parser::parse_schema::<String>(schema)
}
