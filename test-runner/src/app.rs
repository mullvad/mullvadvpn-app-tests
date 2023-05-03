use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

use test_rpc::{AppTrace, Error};

#[cfg(target_os = "windows")]
pub fn find_traces() -> Result<Vec<AppTrace>, Error> {
    // TODO: Check GUI data
    // TODO: Check temp data
    // TODO: Check devices and drivers

    let settings_dir = mullvad_paths::get_default_settings_dir().map_err(|error| {
        log::error!("Failed to obtain system app data: {error}");
        Error::Syscall
    })?;

    let mut traces = vec![
        Path::new(r"C:\Program Files\Mullvad VPN"),
        // NOTE: This only works as of `499c06decda37dc639e5f` in the Mullvad app.
        // Older builds have no way of silently fully uninstalling the app.
        Path::new(r"C:\ProgramData\Mullvad VPN"),
        // NOTE: Works as of `4116ebc` (Mullvad app).
        &settings_dir,
    ];

    filter_non_existent_paths(&mut traces)?;

    Ok(traces
        .into_iter()
        .map(|path| AppTrace::Path(path.to_path_buf()))
        .collect())
}

#[cfg(target_os = "linux")]
pub fn find_traces() -> Result<Vec<AppTrace>, Error> {
    // TODO: Check GUI data
    // TODO: Check temp data

    let mut traces = vec![
        Path::new(r"/etc/mullvad-vpn/"),
        Path::new(r"/var/log/mullvad-vpn/"),
        Path::new(r"/var/cache/mullvad-vpn/"),
        Path::new(r"/opt/Mullvad VPN/"),
        // management interface socket
        Path::new(r"/var/run/mullvad-vpn"),
        // service unit config files
        Path::new(r"/usr/lib/systemd/system/mullvad-daemon.service"),
        Path::new(r"/usr/lib/systemd/system/mullvad-early-boot-blocking.service"),
        Path::new(r"/usr/bin/mullvad"),
        Path::new(r"/usr/bin/mullvad-daemon"),
        Path::new(r"/usr/bin/mullvad-exclude"),
        Path::new(r"/usr/bin/mullvad-problem-report"),
        Path::new(r"/usr/share/bash-completion/completions/mullvad"),
        Path::new(r"/usr/local/share/zsh/site-functions/_mullvad"),
        Path::new(r"/usr/share/fish/vendor_completions.d/mullvad.fish"),
    ];

    filter_non_existent_paths(&mut traces)?;

    Ok(traces
        .into_iter()
        .map(|path| AppTrace::Path(path.to_path_buf()))
        .collect())
}

#[cfg(target_os = "macos")]
pub fn find_traces() -> Result<Vec<AppTrace>, Error> {
    unimplemented!()
}

fn filter_non_existent_paths(paths: &mut Vec<&Path>) -> Result<(), Error> {
    for i in (0..paths.len()).rev() {
        let path_exists = paths[i].try_exists().map_err(|error| {
            log::error!("Failed to check whether path exists: {error}");
            Error::Syscall
        })?;
        if !path_exists {
            paths.swap_remove(i);
            continue;
        }
    }
    Ok(())
}

/// Contains account specific wireguard data
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct WireguardData {
    private_key: serde_json::Value,
    addresses: serde_json::Value,
    created: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct PrivateDevice {
    id: serde_json::Value,
    name: serde_json::Value,
    wg_data: WireguardData,
    ports: serde_json::Value,
    hijack_dns: serde_json::Value,
    created: DateTime<Utc>,
}

/// Same as [PrivateDevice] but also contains the associated account token.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct PrivateAccountAndDevice {
    account_token: serde_json::Value,
    device: PrivateDevice,
}

/// Contains the current device state.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PrivateDeviceState {
    LoggedIn(PrivateAccountAndDevice),
    LoggedOut,
    Revoked,
}

pub async fn make_device_json_old() -> Result<(), Error> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const DEVICE_JSON_PATH: &str = "/etc/mullvad-vpn/device.json";
    #[cfg(target_os = "windows")]
    const DEVICE_JSON_PATH: &str =
        "C:\\Windows\\system32\\config\\systemprofile\\AppData\\Local\\Mullvad VPN\\device.json";
    let device_json = tokio::fs::read_to_string(DEVICE_JSON_PATH)
        .await
        .map_err(|e| Error::FileSystem(e.to_string()))?;

    let mut device_state: serde_json::Value =
        serde_json::from_str(&device_json).map_err(|e| Error::FileSerialization(e.to_string()))?;
    let created_ref: &mut serde_json::Value = device_state
        .get_mut("logged_in")
        .unwrap()
        .get_mut("device")
        .unwrap()
        .get_mut("wg_data")
        .unwrap()
        .get_mut("created")
        .unwrap();
    let created: DateTime<Utc> = serde_json::from_value(created_ref.clone()).unwrap();
    let created = created
        .checked_sub_signed(chrono::Duration::days(365))
        .unwrap();

    *created_ref = serde_json::to_value(created).unwrap();

    let device_json = serde_json::to_string(&device_state)
        .map_err(|e| Error::FileSerialization(e.to_string()))?;
    tokio::fs::write(DEVICE_JSON_PATH, device_json.as_bytes())
        .await
        .map_err(|e| Error::FileSystem(e.to_string()))?;

    Ok(())
}
