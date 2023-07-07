use graphql_parser::{query, schema};

pub fn parse_schema(schema: &str) -> Result<schema::Document<String>, schema::ParseError> {
    schema::parse_schema::<String>(schema)
}

pub fn parse_query(query: &str) -> Result<query::Document<String>, query::ParseError> {
    query::parse_query::<String>(query)
}
