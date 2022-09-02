// TODO: install clean app (& fetch from )
// TODO: update/replace app
// TODO: integrity check

use hyper::{client::Client, Uri};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    str::FromStr,
};
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

    #[error(display = "Failed to run package installer")]
    RunApp,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize, Serialize)]
pub struct Package {
    r#type: PackageType,
    path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum PackageType {
    Dpkg,
    Rpm,
    NsisExe,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InstallResult(Option<i32>);

pub async fn install_package(package: Package) -> Result<InstallResult> {
    // TODO: stdout + stderr?
    match package.r#type {
        PackageType::Dpkg => install_dpkg(&package.path).await,
        PackageType::Rpm => unimplemented!(),
        PackageType::NsisExe => install_nsis_exe(&package.path).await,
    }
}

pub async fn install_dpkg(path: &Path) -> Result<InstallResult> {
    // TODO: find bin
    let mut cmd = Command::new("dpkg");
    cmd.args([OsStr::new("-i"), path.as_os_str()]);
    cmd.kill_on_drop(true);
    cmd.spawn()
        .map_err(|e| strip_error(Error::RunApp, e))?
        .wait()
        .await
        .map(|status| InstallResult(status.code()))
        .map_err(|e| strip_error(Error::RunApp, e))
}

pub async fn install_nsis_exe(path: &Path) -> Result<InstallResult> {
    let mut cmd = Command::new(path);
    
    cmd.kill_on_drop(true);

    // Run the installer in silent mode
    cmd.arg("/S");

    cmd.spawn()
        .map_err(|e| strip_error(Error::RunApp, e))?
        .wait()
        .await
        .map(|code| InstallResult(code.code()))
        .map_err(|e| strip_error(Error::RunApp, e))
}

pub async fn download_app_version(version: &str) -> Result<PathBuf> {
    // TODO: impl correctly for all platforms

    #[cfg(target_os = "windows")]
    let uri = Uri::from_str(&format!(
        "https://releases.mullvad.net/builds/{version}/MullvadVPN-{version}.exe"
    ))
    .map_err(|_error| Error::InvalidVersion)?;

    // TODO: rpm or deb
    // TODO: architecture
    #[cfg(target_os = "linux")]
    let uri = Uri::from_str(&format!(
        "https://releases.mullvad.net/builds/{version}/MullvadVPN-{version}_amd64.deb"
    ))
    .map_err(|_error| Error::InvalidVersion)?;

    download_file(uri).await
}

pub async fn download_file(url: Uri) -> Result<PathBuf> {
    // TODO: save to temporary path
    let mut target_path = PathBuf::new();
    target_path.push(
        Path::new(url.path())
            .file_name()
            .unwrap_or(OsStr::new("unknown"))
            .to_owned(),
    );

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

    let bytes = hyper::body::to_bytes(body)
        .await
        .map_err(|e| strip_error(Error::ToBytes, e))?;

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
