use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Output {
    StdOut(String),
    StdErr(String),
}

impl std::fmt::Display for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Output::StdOut(s) => f.write_str(s),
            Output::StdErr(s) => f.write_str(s),
        }
    }
}
