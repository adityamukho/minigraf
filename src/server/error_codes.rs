use std::fmt::Display;

pub enum ErrorCode {
    InvalidSchemaFile = 1,
    InvalidSchema = 2,
    BadQuery = 3,
}

impl Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let error_code = match self {
            ErrorCode::InvalidSchemaFile => "InvalidSchemaFile",
            ErrorCode::InvalidSchema => "InvalidSchema",
            ErrorCode::BadQuery => "BadQuery",
        };
        write!(f, "{}", error_code)
    }
}
