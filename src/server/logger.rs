use std::fmt::Display;

pub fn error_log<E>(error_code: i32, message: &str, err: E)
where
    E: Display,
{
    eprintln!("ERROR {}: {}: {}", error_code, message, err);
}
