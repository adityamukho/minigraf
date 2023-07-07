use std::env;
use std::fmt::{Debug, Display};
use std::str::FromStr;

use lazy_static::lazy_static;
use crate::server::error_codes::ErrorCode;

#[derive(PartialEq, PartialOrd)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl FromStr for LogLevel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Error" => Ok(LogLevel::Error),
            "Warn" => Ok(LogLevel::Warn),
            "Info" => Ok(LogLevel::Info),
            "Debug" => Ok(LogLevel::Debug),
            "Trace" => Ok(LogLevel::Trace),
            _ => Err(()),
        }
    }
}

lazy_static! {
    static ref LOG_LEVEL: LogLevel = {
        let log_level = env::var("LOG_LEVEL").unwrap_or("Info".to_string());
        log_level.parse().unwrap()
    };
}

pub fn error_log<E>(error_code: ErrorCode, message: &str, err: E)
where
    E: Display,
{
    eprintln!("ERROR {}: {}: {}", error_code, message, err);
}

pub fn debug_log<T>(key: &str, object: &T)
where
    T: Debug,
{
    if *LOG_LEVEL < LogLevel::Debug {
        return;
    }

    println!("DEBUG {}: {:#?}", key, object);
}
