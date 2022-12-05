use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

pub mod logging;
pub mod meta;
pub mod mullvad_daemon;
pub mod package;
pub mod transport;

#[derive(err_derive::Error, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Error {
    #[error(display = "Syscall failed")]
    Syscall,
    #[error(display = "Interface not found")]
    InterfaceNotFound,
    #[error(display = "HTTP request failed")]
    HttpRequest(String),
    #[error(display = "Failed to deserialize HTTP body")]
    DeserializeBody,
    #[error(display = "DNS resolution failed")]
    DnsResolution,
    #[error(display = "Test runner RPC timed out")]
    TestRunnerTimeout,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
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

#[derive(Debug, Serialize, Deserialize)]
pub enum AppTrace {
    Path(PathBuf),
}

#[tarpc::service]
pub trait Service {
    /// Install app package.
    async fn install_app(package_path: package::Package) -> package::Result<()>;

    /// Remove app package.
    async fn uninstall_app() -> package::Result<()>;

    /// Get the output of the runners stdout logs since the last time this function was called.
    /// Block if there is no output until some output is provided by the runner.
    async fn poll_output() -> logging::Result<Vec<logging::Output>>;

    /// Get the output of the runners stdout logs since the last time this function was called.
    /// Block if there is no output until some output is provided by the runner.
    async fn try_poll_output() -> logging::Result<Vec<logging::Output>>;

    async fn get_mullvad_app_logs() -> logging::LogOutput;

    /// Return the OS of the guest.
    async fn get_os() -> meta::Os;

    /// Return status of the system service.
    async fn mullvad_daemon_get_status() -> mullvad_daemon::ServiceStatus;

    /// Returns all Mullvad app files, directories, and other data found on the system.
    async fn find_mullvad_app_traces() -> Result<Vec<AppTrace>, Error>;

    /// Send TCP packet
    async fn send_tcp(bind_addr: SocketAddr, destination: SocketAddr) -> Result<(), ()>;

    /// Send UDP packet
    async fn send_udp(bind_addr: SocketAddr, destination: SocketAddr) -> Result<(), ()>;

    /// Send ICMP
    async fn send_ping(interface: Option<Interface>, destination: IpAddr) -> Result<(), ()>;

    /// Fetch the current location.
    async fn geoip_lookup() -> Result<AmIMullvad, Error>;

    /// Returns the IP of the given interface.
    async fn get_interface_ip(interface: Interface) -> Result<IpAddr, Error>;

    /// Perform DNS resolution.
    async fn resolve_hostname(hostname: String) -> Result<Vec<SocketAddr>, Error>;
}
