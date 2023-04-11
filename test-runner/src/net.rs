use std::{
    net::{IpAddr, SocketAddr},
    process::Output,
};
use test_rpc::Interface;
use tokio::{
    io::AsyncWriteExt,
    net::{TcpSocket, UdpSocket},
    process::Command,
};

#[cfg(target_os = "linux")]
const TUNNEL_INTERFACE: &str = "wg-mullvad";

#[cfg(target_os = "windows")]
const TUNNEL_INTERFACE: &str = "Mullvad";

#[cfg(target_os = "macos")]
const TUNNEL_INTERFACE: &str = "utun3";

pub async fn send_tcp(
    bind_interface: Option<Interface>,
    bind_addr: SocketAddr,
    destination: SocketAddr,
) -> Result<(), test_rpc::Error> {
    let socket = match &destination {
        SocketAddr::V4(_) => TcpSocket::new_v4(),
        SocketAddr::V6(_) => TcpSocket::new_v6(),
    }
    .map_err(|error| {
        log::error!("Failed to create TCP socket: {error}");
        test_rpc::Error::SendTcp
    })?;

    if let Some(iface) = bind_interface {
        let iface = get_interface_name(iface);

        // TODO: macos

        #[cfg(target_os = "linux")]
        socket
            .bind_device(Some(iface.as_bytes()))
            .map_err(|error| {
                log::error!("Failed to bind TCP socket to {iface}: {error}");
                test_rpc::Error::SendTcp
            })?;

        #[cfg(windows)]
        log::trace!("Bind interface {iface} is ignored on Windows")
    }

    socket.bind(bind_addr).map_err(|error| {
        log::error!("Failed to bind TCP socket to {bind_addr}: {error}");
        test_rpc::Error::SendTcp
    })?;

    log::debug!("Connecting from {bind_addr} to {destination}/TCP");

    let mut stream = socket.connect(destination).await.map_err(|error| {
        log::error!("Failed to connect to {destination}: {error}");
        test_rpc::Error::SendTcp
    })?;

    stream.write_all(b"hello").await.map_err(|error| {
        log::error!("Failed to send message to {destination}: {error}");
        test_rpc::Error::SendTcp
    })?;

    Ok(())
}

pub async fn send_udp(
    bind_interface: Option<Interface>,
    bind_addr: SocketAddr,
    destination: SocketAddr,
) -> Result<(), test_rpc::Error> {
    let socket = UdpSocket::bind(bind_addr).await.map_err(|error| {
        log::error!("Failed to bind UDP socket to {bind_addr}: {error}");
        test_rpc::Error::SendUdp
    })?;

    if let Some(iface) = bind_interface {
        let iface = get_interface_name(iface);

        // TODO: macos

        #[cfg(target_os = "linux")]
        socket
            .bind_device(Some(iface.as_bytes()))
            .map_err(|error| {
                log::error!("Failed to bind UDP socket to {iface}: {error}");
                test_rpc::Error::SendUdp
            })?;

        #[cfg(windows)]
        log::trace!("Bind interface {iface} is ignored on Windows")
    }

    log::debug!("Send message from {bind_addr} to {destination}/UDP");

    socket
        .send_to(b"hello", destination)
        .await
        .map_err(|error| {
            log::error!("Failed to send message to {destination}: {error}");
            test_rpc::Error::SendUdp
        })?;

    Ok(())
}

pub async fn send_ping(
    interface: Option<Interface>,
    destination: IpAddr,
) -> Result<(), test_rpc::Error> {
    #[cfg(target_os = "windows")]
    let mut source_ip = None;
    #[cfg(target_os = "windows")]
    if let Some(interface) = interface {
        let family = match destination {
            IpAddr::V4(_) => talpid_windows_net::AddressFamily::Ipv4,
            IpAddr::V6(_) => talpid_windows_net::AddressFamily::Ipv6,
        };
        source_ip = get_interface_ip_for_family(interface, family)
            .map_err(|_error| test_rpc::Error::Syscall)?;
        if source_ip.is_none() {
            log::error!("Failed to obtain interface IP");
            return Err(test_rpc::Error::Ping);
        }
    }

    let mut cmd = Command::new("ping");
    cmd.arg(destination.to_string());

    #[cfg(target_os = "windows")]
    cmd.args(["-n", "1"]);

    #[cfg(not(target_os = "windows"))]
    cmd.args(["-c", "1"]);

    match interface {
        Some(Interface::Tunnel) => {
            log::info!("Pinging {destination} in tunnel");

            #[cfg(target_os = "windows")]
            if let Some(source_ip) = source_ip {
                cmd.args(["-S", &source_ip.to_string()]);
            }

            #[cfg(target_os = "windows")]
            cmd.args(["-I", TUNNEL_INTERFACE]);

            #[cfg(target_os = "macos")]
            cmd.args(["-b", TUNNEL_INTERFACE]);
        }
        Some(Interface::NonTunnel) => {
            log::info!("Pinging {destination} outside tunnel");

            #[cfg(target_os = "windows")]
            if let Some(source_ip) = source_ip {
                cmd.args(["-S", &source_ip.to_string()]);
            }

            #[cfg(target_os = "linux")]
            cmd.args(["-I", non_tunnel_interface()]);

            #[cfg(target_os = "macos")]
            cmd.args(["-b", non_tunnel_interface()]);
        }
        None => log::info!("Pinging {destination}"),
    }

    cmd.kill_on_drop(true);

    cmd.spawn()
        .map_err(|error| {
            log::error!("Failed to spawn ping process: {error}");
            test_rpc::Error::Ping
        })?
        .wait_with_output()
        .await
        .map_err(|error| {
            log::error!("Failed to wait on ping: {error}");
            test_rpc::Error::Ping
        })
        .and_then(|output| result_from_output("ping", output, test_rpc::Error::Ping))
}

#[cfg(unix)]
pub fn get_interface_ip(interface: Interface) -> Result<IpAddr, test_rpc::Error> {
    // TODO: IPv6
    use std::net::Ipv4Addr;

    let alias = get_interface_name(interface);

    let addrs = nix::ifaddrs::getifaddrs().map_err(|error| {
        log::error!("Failed to obtain interfaces: {}", error);
        test_rpc::Error::Syscall
    })?;
    for addr in addrs {
        if addr.interface_name == alias {
            if let Some(address) = addr.address {
                if let Some(sockaddr) = address.as_sockaddr_in() {
                    return Ok(IpAddr::V4(Ipv4Addr::from(sockaddr.ip())));
                }
            }
        }
    }

    log::error!("Could not find tunnel interface");
    Err(test_rpc::Error::InterfaceNotFound)
}

pub fn get_interface_name(interface: Interface) -> &'static str {
    match interface {
        Interface::Tunnel => TUNNEL_INTERFACE,
        Interface::NonTunnel => non_tunnel_interface(),
    }
}

#[cfg(target_os = "windows")]
pub fn get_interface_ip(interface: Interface) -> Result<IpAddr, test_rpc::Error> {
    // TODO: IPv6

    get_interface_ip_for_family(interface, talpid_windows_net::AddressFamily::Ipv4)
        .map_err(|_error| test_rpc::Error::Syscall)?
        .ok_or(test_rpc::Error::InterfaceNotFound)
}

#[cfg(target_os = "windows")]
fn get_interface_ip_for_family(
    interface: Interface,
    family: talpid_windows_net::AddressFamily,
) -> Result<Option<IpAddr>, ()> {
    let interface = match interface {
        Interface::NonTunnel => non_tunnel_interface(),
        Interface::Tunnel => TUNNEL_INTERFACE,
    };
    let interface_alias = talpid_windows_net::luid_from_alias(interface).map_err(|error| {
        log::error!("Failed to obtain interface LUID: {error}");
    })?;

    talpid_windows_net::get_ip_address_for_interface(family, interface_alias).map_err(|error| {
        log::error!("Failed to obtain interface IP: {error}");
    })
}

#[cfg(target_os = "windows")]
fn non_tunnel_interface() -> &'static str {
    use once_cell::sync::OnceCell;
    use talpid_platform_metadata::WindowsVersion;

    static WINDOWS_VERSION: OnceCell<WindowsVersion> = OnceCell::new();
    let version = WINDOWS_VERSION
        .get_or_init(|| WindowsVersion::new().expect("failed to obtain Windows version"));

    if version.build_number() >= 22000 {
        // Windows 11
        return "Ethernet";
    }

    "Ethernet Instance 0"
}

#[cfg(target_os = "linux")]
fn non_tunnel_interface() -> &'static str {
    "ens3"
}

#[cfg(target_os = "macos")]
fn non_tunnel_interface() -> &'static str {
    "en0"
}

fn result_from_output<E>(action: &'static str, output: Output, err: E) -> Result<(), E> {
    if output.status.success() {
        return Ok(());
    }

    let stdout_str = std::str::from_utf8(&output.stdout).unwrap_or("non-utf8 string");
    let stderr_str = std::str::from_utf8(&output.stderr).unwrap_or("non-utf8 string");

    log::error!(
        "{action} failed:\n\ncode: {:?}\n\nstdout:\n\n{}\n\nstderr:\n\n{}",
        output.status.code(),
        stdout_str,
        stderr_str
    );
    Err(err)
}
