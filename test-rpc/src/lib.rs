use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

pub mod logging;
pub mod meta;
pub mod mullvad_daemon;
pub mod package;
pub mod transport;

#[derive(err_derive::Error, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Error {
    #[error(display = "HTTP request failed")]
    HttpRequest(String),
    #[error(display = "Failed to deserialize HTTP body")]
    DeserializeBody,
    #[error(display = "DNS resolution failed")]
    DnsResolution,
    #[error(display = "Test runner RPC timed out")]
    TestRunnerTimeout,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Interface {
    Tunnel,
    NonTunnel,
}

/// Response from am.i.mullvad.net
#[derive(Debug, Serialize, Deserialize)]
pub struct AmIMullvad {
    pub ip: IpAddr,
    pub mullvad_exit_ip: bool,
    pub mullvad_exit_ip_hostname: String,
}

#[tarpc::service]
pub trait Service {
    /// Install app package.
    async fn install_app(package_path: package::Package)
        -> package::Result<package::InstallResult>;

    /// Remove app package.
    async fn uninstall_app() -> package::Result<package::InstallResult>;

    async fn poll_output() -> mullvad_daemon::Result<Vec<logging::Output>>;

    async fn try_poll_output() -> mullvad_daemon::Result<Vec<logging::Output>>;

    /// Return the OS of the guest.
    async fn get_os() -> meta::Os;

    /// Return status of the system service.
    async fn mullvad_daemon_get_status() -> mullvad_daemon::ServiceStatus;

    /// Send ICMP
    async fn send_ping(interface: Option<Interface>, destination: IpAddr) -> Result<(), ()>;

    /// Fetch the current location.
    async fn geoip_lookup() -> Result<AmIMullvad, Error>;

    /// Perform DNS resolution.
    async fn resolve_hostname(hostname: String) -> Result<Vec<SocketAddr>, Error>;
}
