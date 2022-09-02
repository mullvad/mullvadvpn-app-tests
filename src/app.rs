// TODO: install clean app (& fetch from )
// TODO: update/replace app
// TODO: integrity check

use hyper::{client::Client, Uri};
use std::{ffi::OsStr, path::Path, str::FromStr};
use tokio::{fs, io};

#[derive(Debug)]
pub enum Error {
    NotFound,
    WriteFile(io::Error),
    OpenFile(io::Error),
    ToBytes(hyper::Error),
    RequestFailed(hyper::Error),
    InvalidVersion,
}

pub type Result<T> = std::result::Result<T, Error>;

pub async fn download_app_version(version: &str) -> Result<()> {
    // TODO: impl correctly for all platforms

    let uri = Uri::from_str(&format!(
        "https://releases.mullvad.net/builds/{version}/MullvadVPN-{version}.exe"
    ))
    .map_err(|_error| Error::InvalidVersion)?;

    download_file(uri).await
}

pub async fn download_file(url: Uri) -> Result<()> {
    // TODO: save to temporary path
    let target_path = Path::new(url.path())
        .file_name()
        .unwrap_or(OsStr::new("unknown"))
        .to_owned();

    println!("Downloading {url} to {}", target_path.to_string_lossy());

    // TODO: pin certificate
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_only()
        .enable_http1()
        .build();

    let client: Client<_, hyper::Body> = Client::builder().build(https);
    let body = client
        .get(url)
        .await
        .map_err(Error::RequestFailed)?
        .into_body();

    // TODO: limit file size

    let bytes = hyper::body::to_bytes(body).await.map_err(Error::ToBytes)?;

    fs::write(target_path, &bytes)
        .await
        .map_err(Error::WriteFile)
}
