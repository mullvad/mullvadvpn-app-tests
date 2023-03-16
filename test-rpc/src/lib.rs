use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

pub mod client;
pub mod logging;
pub mod meta;
pub mod mullvad_daemon;
pub mod net;
pub mod package;
pub mod transport;

#[derive(err_derive::Error, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Error {
    #[error(display = "Test runner RPC failed")]
    Tarpc(#[error(source)] tarpc::client::RpcError),
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
    #[error(display = "Package error")]
    Package(#[error(source)] package::Error),
    #[error(display = "Logger error")]
    Logger(#[error(source)] logging::Error),
    #[error(display = "Failed to send UDP datagram")]
    SendUdp,
    #[error(display = "Failed to send TCP segment")]
    SendTcp,
    #[error(display = "Failed to send ping")]
    Ping,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExecResult {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ExecResult {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AppTrace {
    Path(PathBuf),
}

mod service {
    pub use super::*;

    #[tarpc::service]
    pub trait Service {
        /// Install app package.
        async fn install_app(package_path: package::Package) -> Result<(), Error>;

        /// Remove app package.
        async fn uninstall_app() -> Result<(), Error>;

        /// Execute a program.
        async fn exec(path: String, args: Vec<String>) -> Result<ExecResult, Error>;

        /// Get the output of the runners stdout logs since the last time this function was called.
        /// Block if there is no output until some output is provided by the runner.
        async fn poll_output() -> Result<Vec<logging::Output>, Error>;

        /// Get the output of the runners stdout logs since the last time this function was called.
        /// Block if there is no output until some output is provided by the runner.
        async fn try_poll_output() -> Result<Vec<logging::Output>, Error>;

        async fn get_mullvad_app_logs() -> logging::LogOutput;

        /// Return the OS of the guest.
        async fn get_os() -> meta::Os;

        /// Return status of the system service.
        async fn mullvad_daemon_get_status() -> mullvad_daemon::ServiceStatus;

        /// Returns all Mullvad app files, directories, and other data found on the system.
        async fn find_mullvad_app_traces() -> Result<Vec<AppTrace>, Error>;

        /// Send TCP packet
        async fn send_tcp(
            interface: Option<Interface>,
            bind_addr: SocketAddr,
            destination: SocketAddr,
        ) -> Result<(), Error>;

        /// Send UDP packet
        async fn send_udp(
            interface: Option<Interface>,
            bind_addr: SocketAddr,
            destination: SocketAddr,
        ) -> Result<(), Error>;

        /// Send ICMP
        async fn send_ping(interface: Option<Interface>, destination: IpAddr) -> Result<(), Error>;

        /// Fetch the current location.
        async fn geoip_lookup() -> Result<AmIMullvad, Error>;

        /// Returns the IP of the given interface.
        async fn get_interface_ip(interface: Interface) -> Result<IpAddr, Error>;

        /// Perform DNS resolution.
        async fn resolve_hostname(hostname: String) -> Result<Vec<SocketAddr>, Error>;

        async fn reboot() -> Result<(), Error>;
    }
}

pub use client::ServiceClient;
pub use service::{Service, ServiceRequest, ServiceResponse};
