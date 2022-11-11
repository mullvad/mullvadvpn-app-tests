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

    Ok(traces.into_iter().map(|path| AppTrace::Path(path.to_path_buf())).collect())
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

    Ok(traces.into_iter().map(|path| AppTrace::Path(path.to_path_buf())).collect())
}

#[cfg(target_os = "macos")]
pub fn find_traces() -> Result<Vec<AppTrace>, Error> {
    unimplemented!()
}

fn filter_non_existent_paths(paths: &mut Vec<&Path>) -> Result<(), Error> {
    for i in (0..paths.len()).rev() {
        if !paths[i].try_exists().map_err(|error| {
            log::error!("Failed to check whether path exists: {error}");
            Error::Syscall
        })? {
            paths.swap_remove(i);
            continue;
        }
    }
    Ok(())
}
