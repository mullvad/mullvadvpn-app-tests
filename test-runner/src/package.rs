// TODO: Fix terrible abstraction

use std::{ffi::OsStr, path::Path};
use test_rpc::package::{Error, InstallResult, Package, PackageType, Result};
use tokio::process::Command;

#[cfg(target_os = "linux")]
pub async fn uninstall_app() -> Result<InstallResult> {
    // TODO: Fedora
    // TODO: Consider using: dpkg -r $(dpkg -f package.deb Package)
    uninstall_dpkg("mullvad-vpn", true).await
}
#[cfg(target_os = "macos")]
pub async fn uninstall_app() -> Result<InstallResult> {
    unimplemented!()
}

#[cfg(target_os = "windows")]
pub async fn uninstall_app() -> Result<InstallResult> {
    // TODO: obtain from registry
    // TODO: can this mimic an actual uninstall more closely?

    let program_dir = Path::new(r"C:\Program Files\Mullvad VPN");
    let uninstall_path = program_dir.join("Uninstall Mullvad VPN.exe");

    // To wait for the uninstaller, we must copy it to a temporary directory and
    // supply it with the install path.

    let temp_uninstaller = std::env::temp_dir().join("mullvad_uninstall.exe");
    tokio::fs::copy(uninstall_path, &temp_uninstaller)
        .await
        .map_err(|e| strip_error(Error::CreateTempUninstaller, e))?;

    let mut cmd = Command::new(temp_uninstaller);

    cmd.kill_on_drop(true);
    cmd.arg("/allusers");
    // Silent mode
    cmd.arg("/S");
    // NSIS! Doesn't understand that it shouldn't fork itself unless
    // there's whitespace prepended to "_?=".
    cmd.arg(format!(" _?={}", program_dir.display()));

    cmd.spawn()
        .map_err(|e| strip_error(Error::RunApp, e))?
        .wait()
        .await
        .map(|code| InstallResult(code.code()))
        .map_err(|e| strip_error(Error::RunApp, e))
}

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

#[cfg(target_os = "linux")]
async fn uninstall_dpkg(name: &str, purge: bool) -> Result<InstallResult> {
    let mut cmd = Command::new("/usr/bin/dpkg");
    if purge {
        cmd.args([OsStr::new("--purge"), OsStr::new(name)]);
    } else {
        cmd.args([OsStr::new("-r"), OsStr::new(name)]);
    }
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
