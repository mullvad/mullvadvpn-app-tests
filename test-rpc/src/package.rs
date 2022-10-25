use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(err_derive::Error, Debug, Deserialize, Serialize)]
#[error(no_from)]
pub enum Error {
    #[error(display = "Failed open file for writing")]
    OpenFile,

    #[error(display = "Failed to write downloaded file to disk")]
    WriteFile,

    #[error(display = "Failed to convert download to bytes")]
    ToBytes,

    #[error(display = "Failed to convert download to bytes")]
    RequestFailed,

    #[error(display = "Cannot parse version")]
    InvalidVersion,

    #[error(display = "Failed to run package installer")]
    RunApp,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize, Serialize)]
pub struct Package {
    pub r#type: PackageType,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum PackageType {
    Dpkg,
    Rpm,
    NsisExe,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InstallResult(pub Option<i32>);