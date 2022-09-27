use serde::{Deserialize, Serialize};
use std::path::Path;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const SOCKET_PATH: &str = "/var/run/mullvad-vpn";
#[cfg(windows)]
const SOCKET_PATH: &str = "//./pipe/Mullvad VPN";

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
