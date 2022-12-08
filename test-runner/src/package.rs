// TODO: Fix terrible abstraction

use std::{
    ffi::OsStr,
    path::Path,
    process::{Output, Stdio},
};
use test_rpc::package::{Error, Package, PackageType, Result};
use tokio::process::Command;

#[cfg(target_os = "linux")]
pub async fn uninstall_app() -> Result<()> {
    // TODO: Fedora
    // TODO: Consider using: dpkg -r $(dpkg -f package.deb Package)
    uninstall_dpkg("mullvad-vpn", true).await
}
#[cfg(target_os = "macos")]
pub async fn uninstall_app() -> Result<()> {
    unimplemented!()
}

#[cfg(target_os = "windows")]
pub async fn uninstall_app() -> Result<()> {
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
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    cmd.spawn()
        .map_err(|e| strip_error(Error::RunApp, e))?
        .wait_with_output()
        .await
        .map_err(|e| strip_error(Error::RunApp, e))
        .and_then(|output| result_from_output("uninstall app", output))
}

pub async fn install_package(package: Package) -> Result<()> {
    match package.r#type {
        PackageType::Dpkg => install_dpkg(&package.path).await,
        PackageType::Rpm => unimplemented!(),
        PackageType::NsisExe => install_nsis_exe(&package.path).await,
    }
}

async fn install_dpkg(path: &Path) -> Result<()> {
    let mut cmd = Command::new("/usr/bin/dpkg");
    cmd.args([OsStr::new("-i"), path.as_os_str()]);
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.spawn()
        .map_err(|e| strip_error(Error::RunApp, e))?
        .wait_with_output()
        .await
        .map_err(|e| strip_error(Error::RunApp, e))
        .and_then(|output| result_from_output("dpkg -i", output))
}

#[cfg(target_os = "linux")]
async fn uninstall_dpkg(name: &str, purge: bool) -> Result<()> {
    let action;
    let mut cmd = Command::new("/usr/bin/dpkg");
    if purge {
        action = "dpkg --purge";
        cmd.args(["--purge", name]);
    } else {
        action = "dpkg -r";
        cmd.args(["-r", name]);
    }
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.spawn()
        .map_err(|e| strip_error(Error::RunApp, e))?
        .wait_with_output()
        .await
        .map_err(|e| strip_error(Error::RunApp, e))
        .and_then(|output| result_from_output(action, output))
}

async fn install_nsis_exe(path: &Path) -> Result<()> {
    let mut cmd = Command::new(path);

    cmd.kill_on_drop(true);

    // Run the installer in silent mode
    cmd.arg("/S");

    cmd.spawn()
        .map_err(|e| strip_error(Error::RunApp, e))?
        .wait_with_output()
        .await
        .map_err(|e| strip_error(Error::RunApp, e))
        .and_then(|output| result_from_output("install app", output))
}

fn strip_error<T: std::error::Error>(error: Error, source: T) -> Error {
    log::error!("Error: {error}\ncause: {source}");
    error
}

fn result_from_output(action: &'static str, output: Output) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }

    let stdout_str = std::str::from_utf8(&output.stdout).unwrap_or("non-utf8 string");
    let stderr_str = std::str::from_utf8(&output.stderr).unwrap_or("non-utf8 string");

    log::error!(
        "{action} failed:\n\nstdout:\n\n{}\n\nstderr:\n\n{}",
        stdout_str,
        stderr_str
    );

    Err(output
        .status
        .code()
        .map(Error::InstallerFailed)
        .unwrap_or(Error::InstallerFailedSignal))
}
