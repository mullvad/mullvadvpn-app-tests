use hyper::{Client, Uri};
use serde::de::DeserializeOwned;
use std::net::IpAddr;
use test_rpc::{AmIMullvad, Interface};
use tokio::process::Command;

const LE_ROOT_CERT: &[u8] = include_bytes!("./le_root_cert.pem");

pub async fn send_ping(interface: Option<Interface>, destination: IpAddr) -> Result<(), ()> {
    #[cfg(not(target_os = "windows"))]
    const TUNNEL_INTERFACE: &str = "wg-mullvad";
    #[cfg(not(target_os = "windows"))]
    const NON_TUNNEL_INTERFACE: &str = "ens3";

    #[cfg(target_os = "windows")]
    const TUNNEL_INTERFACE: &str = "Mullvad";
    #[cfg(target_os = "windows")]
    const NON_TUNNEL_INTERFACE: &str = "Ethernet Instance 0";

    #[cfg(target_os = "windows")]
    let mut source_ip = None;
    #[cfg(target_os = "windows")]
    {
        if let Some(interface) = interface.as_ref() {
            let interface = match interface {
                Interface::NonTunnel => NON_TUNNEL_INTERFACE,
                Interface::Tunnel => TUNNEL_INTERFACE,
            };
            let interface_alias =
                talpid_windows_net::luid_from_alias(interface).map_err(|error| {
                    log::error!("Failed to obtain interface LUID: {error}");
                })?;

            let family = match destination {
                IpAddr::V4(_) => talpid_windows_net::AddressFamily::Ipv4,
                IpAddr::V6(_) => talpid_windows_net::AddressFamily::Ipv6,
            };

            source_ip = talpid_windows_net::get_ip_address_for_interface(family, interface_alias)
                .map_err(|error| {
                log::error!("Failed to obtain interface IP: {error}");
            })?;

            if source_ip.is_none() {
                log::error!("Failed to obtain interface IP");
                return Err(());
            }
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
            cmd.args(["-I", NON_TUNNEL_INTERFACE]);
        }
        None => log::info!("Pinging {destination}"),
    }

    cmd.kill_on_drop(true);

    match cmd.spawn().map_err(|_err| ())?.wait().await {
        Ok(_status) if _status.code() == Some(0) => Ok(()),
        _ => Err(()),
    }
}

pub async fn geoip_lookup() -> Result<AmIMullvad, test_rpc::Error> {
    let uri = Uri::from_static("https://ipv4.am.i.mullvad.net/json");
    deserialize_from_http_get(uri).await
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
