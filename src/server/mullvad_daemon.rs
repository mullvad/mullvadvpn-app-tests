use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const SOCKET_PATH: &str = "/var/run/mullvad-vpn";
#[cfg(windows)]
const SOCKET_PATH: &str = "//./pipe/Mullvad VPN";

#[cfg(any(target_os = "linux", target_os = "macos"))]
const MULLVAD_BIN: &str = "mullvad";
#[cfg(windows)]
const MULLVAD_BIN: &str = "mullvad.exe";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Error {
    ConnectError,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceStatus {
    NotRunning,
    Running,
}

// TODO: connect to gRPC service instead
pub fn get_status() -> ServiceStatus {
    match Path::new(SOCKET_PATH).exists() {
        true => ServiceStatus::Running,
        false => ServiceStatus::NotRunning,
    }
}

// FIXME: connect to gRPC service instead
pub async fn connect() -> Result<()> {
    let mut cmd = Command::new(MULLVAD_BIN);

    cmd.kill_on_drop(true);

    cmd.arg("connect");

    match cmd
        .spawn()
        .map_err(|_err| Error::ConnectError)?
        .wait()
        .await
    {
        Ok(_status) if _status.code() == Some(0) => Ok(()),
        _ => Err(Error::ConnectError),
    }
}
