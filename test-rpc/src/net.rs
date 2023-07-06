use hyper::{Client, Uri};
use once_cell::sync::Lazy;
use serde::de::DeserializeOwned;
use tokio_rustls::rustls::ClientConfig;

use crate::{AmIMullvad, Error};

const LE_ROOT_CERT: &[u8] = include_bytes!("./le_root_cert.pem");

static CLIENT_CONFIG: Lazy<ClientConfig> = Lazy::new(|| {
    ClientConfig::builder()
        .with_safe_default_cipher_suites()
        .with_safe_default_kx_groups()
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(read_cert_store())
        .with_no_client_auth()
});

pub async fn geoip_lookup(mullvad_host: String) -> Result<AmIMullvad, Error> {
    let uri = Uri::try_from(format!("https://ipv4.am.i.{mullvad_host}/json"))
        .map_err(|_| Error::InvalidUrl)?;
    http_get(uri).await
}

pub async fn http_get<T: DeserializeOwned>(url: Uri) -> Result<T, Error> {
    log::debug!("GET {url}");

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(CLIENT_CONFIG.clone())
        .https_only()
        .enable_http1()
        .build();

    let client: Client<_, hyper::Body> = Client::builder().build(https);
    let body = client
        .get(url)
        .await
        .map_err(|error| Error::HttpRequest(error.to_string()))?
        .into_body();

    // TODO: limit length
    let bytes = hyper::body::to_bytes(body).await.map_err(|error| {
        log::error!("Failed to convert body to bytes buffer: {}", error);
        Error::DeserializeBody
    })?;

    serde_json::from_slice(&bytes).map_err(|error| {
        log::error!("Failed to deserialize response: {}", error);
        Error::DeserializeBody
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

/// Perform an HTTP GET request. If the request takes too long to finished it
/// will time out and be retried a bounded number of times.
///
/// * `url` - Where to perform the HTTP GET request.
/// * `retries` - Number of times the request will be retried before reporting the check as an
/// error. By default, `retries` is set to 3.
///
/// This function is useful to verify that the tunnel works properly, i.e. that
/// the internet is reachable when traffic is routed through the tunnel.
pub async fn http_get_with_retries<T: DeserializeOwned>(
    url: &str,
    retries: Option<u8>,
) -> Result<T, Error> {
    use std::time::Duration;
    let retries = retries.unwrap_or(3);
    const BEFORE_RETRY_DELAY: Duration = Duration::from_secs(2);

    // Perform the request(s)
    let uri = Uri::try_from(url).map_err(|_| Error::InvalidUrl)?;
    let mut attempt = 0;
    loop {
        let result: Result<T, Error> = http_get(uri.clone()).await;

        attempt += 1;
        if result.is_ok() || attempt >= retries {
            break result;
        }

        tokio::time::sleep(BEFORE_RETRY_DELAY).await;
    }
}
