use std::fmt::{Display, Formatter, Result};

pub enum ErrorCode {
    InvalidSchemaFile = 1,
    InvalidSchema = 2,
    BadQuery = 3,
}

impl Display for ErrorCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let error_code = match self {
            ErrorCode::InvalidSchemaFile => "InvalidSchemaFile",
            ErrorCode::InvalidSchema => "InvalidSchema",
            ErrorCode::BadQuery => "BadQuery",
        };
        write!(f, "{}", error_code)
    }
}
