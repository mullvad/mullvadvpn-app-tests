// TODO: install clean app (& fetch from )
// TODO: update/replace app
// TODO: integrity check

use hyper::{client::Client, Uri};
use serde::{Serialize, Deserialize};
use std::{ffi::OsStr, path::{Path, PathBuf}, str::FromStr};
use tokio::{fs, process::Command};

#[derive(err_derive::Error, Debug, Deserialize, Serialize)]
#[error(no_from)]
pub enum Error {
    #[error(display = "Failed open file for writing")]
    OpenFile,

    #[error(display = "Failed to write downloaded file to disk")]
    WriteFile,

    #[error(display = "Failed to convert download to bytes")]
    ToBytes,

    #[error(display = "Failed to convert download to bytes")]
    RequestFailed,

    #[error(display = "Cannot parse version")]
    InvalidVersion,

    #[error(display = "Failed to run installer package")]
    RunApp,
}

pub type Result<T> = std::result::Result<T, Error>;

pub async fn download_app_version(version: &str) -> Result<PathBuf> {
    // TODO: impl correctly for all platforms

    let uri = Uri::from_str(&format!(
        "https://releases.mullvad.net/builds/{version}/MullvadVPN-{version}.exe"
    ))
    .map_err(|_error| Error::InvalidVersion)?;

    download_file(uri).await
}

pub async fn install_package(path: &Path) -> Result<Option<i32>> {
    // TODO: implement for other OSes
    // TODO: don't execute arbitrary program
    let mut cmd = Command::new(path);
    cmd.kill_on_drop(true);
    cmd.spawn()
        .map_err(|e| strip_error(Error::RunApp, e))?
        .wait()
        .await
        .map(|code| code.code())
        .map_err(|e| strip_error(Error::RunApp, e))
}

pub async fn download_file(url: Uri) -> Result<PathBuf> {
    // TODO: save to temporary path
    let mut target_path = PathBuf::new();
    target_path.push(Path::new(url.path())
        .file_name()
        .unwrap_or(OsStr::new("unknown"))
        .to_owned());

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
        .map_err(|e| strip_error(Error::RequestFailed, e))?
        .into_body();

    // TODO: limit file size

    let bytes = hyper::body::to_bytes(body).await.map_err(|e| strip_error(Error::ToBytes, e))?;

    fs::write(&target_path, &bytes)
        .await
        .map_err(|e| strip_error(Error::WriteFile, e))?;

    // TODO: integrity check

    Ok(target_path)
}

fn strip_error<T: std::error::Error>(error: Error, source: T) -> Error {
    eprintln!("Error: {error}\ncause: {source}");
    error
}
