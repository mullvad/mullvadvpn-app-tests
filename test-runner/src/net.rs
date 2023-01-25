use hyper::{Client, Uri};
use serde::de::DeserializeOwned;
use std::{
    net::{IpAddr, SocketAddr},
    process::Output,
};
use test_rpc::{AmIMullvad, Interface};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpSocket, UdpSocket},
    process::Command,
};

const LE_ROOT_CERT: &[u8] = include_bytes!("./le_root_cert.pem");

#[cfg(not(target_os = "windows"))]
const TUNNEL_INTERFACE: &str = "wg-mullvad";

#[cfg(target_os = "windows")]
const TUNNEL_INTERFACE: &str = "Mullvad";

pub async fn send_tcp(
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
    bind_addr: SocketAddr,
    destination: SocketAddr,
) -> Result<(), test_rpc::Error> {
    let socket = UdpSocket::bind(bind_addr).await.map_err(|error| {
        log::error!("Failed to bind UDP socket to {bind_addr}: {error}");
        test_rpc::Error::SendUdp
    })?;

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
        source_ip = get_interface_ip_for_family(interface, family)?;
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

            #[cfg(not(target_os = "windows"))]
            cmd.args(["-I", TUNNEL_INTERFACE]);
        }
        Some(Interface::NonTunnel) => {
            log::info!("Pinging {destination} outside tunnel");

            #[cfg(target_os = "windows")]
            if let Some(source_ip) = source_ip {
                cmd.args(["-S", &source_ip.to_string()]);
            }

            #[cfg(not(target_os = "windows"))]
            cmd.args(["-I", non_tunnel_interface()]);
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

pub async fn geoip_lookup() -> Result<AmIMullvad, test_rpc::Error> {
    let uri = Uri::from_static("https://ipv4.am.i.mullvad.net/json");
    deserialize_from_http_get(uri).await
}

#[cfg(target_os = "linux")]
pub fn get_interface_ip(interface: Interface) -> Result<IpAddr, test_rpc::Error> {
    // TODO: IPv6
    use std::net::Ipv4Addr;

    let alias = match interface {
        Interface::Tunnel => TUNNEL_INTERFACE,
        Interface::NonTunnel => non_tunnel_interface(),
    };

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

#[cfg(target_os = "windows")]
pub fn get_interface_ip(interface: Interface) -> Result<IpAddr, test_rpc::Error> {
    // TODO: IPv6

    get_interface_ip_for_family(interface, talpid_windows_net::AddressFamily::Ipv4)
        .map_err(|_error| test_rpc::Error::Syscall)?
        .ok_or(test_rpc::Error::InterfaceNotFound)
}

#[cfg(target_os = "macos")]
pub fn get_interface_ip(interface: Interface) -> Result<IpAddr, test_rpc::Error> {
    unimplemented!()
}

async fn deserialize_from_http_get<T: DeserializeOwned>(url: Uri) -> Result<T, test_rpc::Error> {
    log::debug!("GET {url}");

    use tokio_rustls::rustls::ClientConfig;

    let config = ClientConfig::builder()
        .with_safe_default_cipher_suites()
        .with_safe_default_kx_groups()
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(read_cert_store())
        .with_no_client_auth();

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_only()
        .enable_http1()
        .build();

    let client: Client<_, hyper::Body> = Client::builder().build(https);
    let body = client
        .get(url)
        .await
        .map_err(|error| test_rpc::Error::HttpRequest(error.to_string()))?
        .into_body();

    // TODO: limit length
    let bytes = hyper::body::to_bytes(body).await.map_err(|error| {
        log::error!("Failed to convert body to bytes buffer: {}", error);
        test_rpc::Error::DeserializeBody
    })?;

    serde_json::from_slice(&bytes).map_err(|error| {
        log::error!("Failed to deserialize response: {}", error);
        test_rpc::Error::DeserializeBody
    })
}

fn read_cert_store() -> tokio_rustls::rustls::RootCertStore {
    let mut cert_store = tokio_rustls::rustls::RootCertStore::empty();

    let certs = rustls_pemfile::certs(&mut std::io::BufReader::new(LE_ROOT_CERT))
        .expect("Failed to parse pem file");
    let (num_certs_added, num_failures) = cert_store.add_parsable_certificates(&certs);
    if num_failures > 0 || num_certs_added != 1 {
        panic!("Failed to add root cert");
    }

    cert_store
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

#[cfg(not(target_os = "windows"))]
fn non_tunnel_interface() -> &'static str {
    "ens3"
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
