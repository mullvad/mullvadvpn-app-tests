use std::{ffi::OsStr, path::Path};
use test_rpc::package::{Error, InstallResult, Package, PackageType, Result};
use tokio::process::Command;

pub async fn install_package(package: Package) -> Result<InstallResult> {
    match package.r#type {
        PackageType::Dpkg => install_dpkg(&package.path).await,
        PackageType::Rpm => unimplemented!(),
        PackageType::NsisExe => install_nsis_exe(&package.path).await,
    }
}

async fn install_dpkg(path: &Path) -> Result<InstallResult> {
    let mut cmd = Command::new("/usr/bin/dpkg");
    cmd.args([OsStr::new("-i"), path.as_os_str()]);
    cmd.kill_on_drop(true);
    cmd.spawn()
        .map_err(|e| strip_error(Error::RunApp, e))?
        .wait()
        .await
        .map(|status| InstallResult(status.code()))
        .map_err(|e| strip_error(Error::RunApp, e))
}

async fn install_nsis_exe(path: &Path) -> Result<InstallResult> {
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

fn strip_error<T: std::error::Error>(error: Error, source: T) -> Error {
    log::error!("Error: {error}\ncause: {source}");
    error
}
