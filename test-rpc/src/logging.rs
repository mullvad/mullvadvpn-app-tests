use colored::Colorize;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Output {
    StdOut(String),
    StdErr(String),
    Other(String),
}

impl std::fmt::Display for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Output::StdOut(s) => f.write_fmt(format_args!("{}", s.as_str().yellow())),
            Output::StdErr(s) => f.write_fmt(format_args!("{}", s.as_str().red())),
            Output::Other(s) => f.write_fmt(format_args!("{}", s.as_str())),
        }
    }
}
