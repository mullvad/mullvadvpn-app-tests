use serde::{Deserialize, Serialize};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub const SOCKET_PATH: &str = "/var/run/mullvad-vpn";
#[cfg(windows)]
pub const SOCKET_PATH: &str = "//./pipe/Mullvad VPN";

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
