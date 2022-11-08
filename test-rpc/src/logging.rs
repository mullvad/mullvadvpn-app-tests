use colored::Colorize;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Output {
    Error(String),
    Warning(String),
    Info(String),
    Other(String),
}

impl std::fmt::Display for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Output::Error(s) => f.write_fmt(format_args!("{}", s.as_str().red())),
            Output::Warning(s) => f.write_fmt(format_args!("{}", s.as_str().yellow())),
            Output::Info(s) => f.write_fmt(format_args!("{}", s.as_str())),
            Output::Other(s) => f.write_fmt(format_args!("{}", s.as_str())),
        }
    }
}
