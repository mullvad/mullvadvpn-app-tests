use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Error {
    ConnectError,
    CanNotGetOutput,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceStatus {
    NotRunning,
    Running,
}
