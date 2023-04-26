#[cfg(target_os = "windows")]
use std::io;

#[cfg(target_os = "windows")]
use windows_service::{
    service::{ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType},
    service_manager::{ServiceManager, ServiceManagerAccess},
};
#[cfg(target_os = "windows")]
use std::ffi::OsString;

#[cfg(target_os = "macos")]
pub fn reboot() -> Result<(), test_rpc::Error> {
    unimplemented!("not implemented")
}

#[cfg(target_os = "windows")]
pub fn reboot() -> Result<(), test_rpc::Error> {
    use windows_sys::Win32::System::Shutdown::{
        ExitWindowsEx, EWX_REBOOT, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_APPLICATION,
        SHTDN_REASON_MINOR_OTHER,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::EWX_FORCEIFHUNG;

    grant_shutdown_privilege()?;

    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(5));

        let shutdown_result = unsafe {
            ExitWindowsEx(
                EWX_FORCEIFHUNG | EWX_REBOOT,
                SHTDN_REASON_MAJOR_APPLICATION
                    | SHTDN_REASON_MINOR_OTHER
                    | SHTDN_REASON_FLAG_PLANNED,
            )
        };

        if shutdown_result == 0 {
            log::error!(
                "Failed to restart test machine: {}",
                io::Error::last_os_error()
            );
            std::process::exit(1);
        }

        std::process::exit(0);
    });

    // NOTE: We do not bother to revoke the privilege.

    Ok(())
}

#[cfg(target_os = "windows")]
fn grant_shutdown_privilege() -> Result<(), test_rpc::Error> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Foundation::LUID;
    use windows_sys::Win32::Security::AdjustTokenPrivileges;
    use windows_sys::Win32::Security::LookupPrivilegeValueW;
    use windows_sys::Win32::Security::LUID_AND_ATTRIBUTES;
    use windows_sys::Win32::Security::SE_PRIVILEGE_ENABLED;
    use windows_sys::Win32::Security::TOKEN_ADJUST_PRIVILEGES;
    use windows_sys::Win32::Security::TOKEN_PRIVILEGES;
    use windows_sys::Win32::System::SystemServices::SE_SHUTDOWN_NAME;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    use windows_sys::Win32::System::Threading::OpenProcessToken;

    let mut privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: LUID {
                HighPart: 0,
                LowPart: 0,
            },
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };

    if unsafe {
        LookupPrivilegeValueW(
            std::ptr::null(),
            SE_SHUTDOWN_NAME,
            &mut privileges.Privileges[0].Luid,
        )
    } == 0
    {
        log::error!(
            "Failed to lookup shutdown privilege LUID: {}",
            io::Error::last_os_error()
        );
        return Err(test_rpc::Error::Syscall);
    }

    let mut token_handle: HANDLE = 0;

    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES,
            &mut token_handle,
        )
    } == 0
    {
        log::error!("OpenProcessToken() failed: {}", io::Error::last_os_error());
        return Err(test_rpc::Error::Syscall);
    }

    let result = unsafe {
        AdjustTokenPrivileges(
            token_handle,
            0,
            &mut privileges,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    unsafe { CloseHandle(token_handle) };

    if result == 0 {
        log::error!(
            "Failed to enable SE_SHUTDOWN_NAME: {}",
            io::Error::last_os_error()
        );
        return Err(test_rpc::Error::Syscall);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn reboot() -> Result<(), test_rpc::Error> {
    log::debug!("Rebooting system");

    std::thread::spawn(|| {
        let mut cmd = std::process::Command::new("/usr/sbin/shutdown");
        cmd.args(["-r", "now"]);

        std::thread::sleep(std::time::Duration::from_secs(5));

        let _ = cmd.spawn().map_err(|error| {
            log::error!("Failed to spawn shutdown command: {error}");
            error
        });
    });

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn set_daemon_log_level(verbosity_level: usize) -> Result<(), test_rpc::Error> {
    const SYSTEMD_SERVICE_FILE: &str = "/lib/systemd/system/mullvad-daemon.service";

    let verbosity = if verbosity_level == 0 {
        ""
    } else if verbosity_level == 1 {
        " -v"
    } else if verbosity_level == 2 {
        " -vv"
    } else {
        " -vvv"
    };
    let systemd_service_file_content = format!(
        r#"# Systemd service unit file for the Mullvad VPN daemon
# testing if new changes are added

[Unit]
Description=Mullvad VPN daemon
Before=network-online.target
After=mullvad-early-boot-blocking.service NetworkManager.service systemd-resolved.service

StartLimitBurst=5
StartLimitIntervalSec=20
RequiresMountsFor=/opt/Mullvad\x20VPN/resources/

[Service]
Restart=always
RestartSec=1
ExecStart=/usr/bin/mullvad-daemon --disable-stdout-timestamps{verbosity}
Environment="MULLVAD_RESOURCE_DIR=/opt/Mullvad VPN/resources/"

[Install]
WantedBy=multi-user.target"#
    );

    std::fs::write(SYSTEMD_SERVICE_FILE, systemd_service_file_content).unwrap();

    std::process::Command::new("systemctl")
        .args(["daemon-reload"])
        .spawn()
        .unwrap();
    std::process::Command::new("systemctl")
        .args(["restart", "mullvad-daemon"])
        .spawn()
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(1000));
    Ok(())
}

//#[cfg(target_os = "windows")]
pub fn set_daemon_log_level(verbosity_level: usize) -> Result<(), test_rpc::Error> {
    log::error!("Setting log level");
    let verbosity = if verbosity_level == 0 {
        ""
    } else if verbosity_level == 1 {
        "-v"
    } else if verbosity_level == 2 {
        "-vv"
    } else {
        "-vvv"
    };

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).unwrap();
    let service = manager.open_service(
        "mullvadvpn",
        ServiceAccess::QUERY_CONFIG | ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::STOP,
    ).unwrap();

    // Stop the service
    service.stop().unwrap();
    std::thread::sleep_ms(1000);
    log::error!("Stopping old service");

    // Get the current service configuration
    let config = service.query_config().unwrap();

    //let old_exec = config.executable_path.display().to_string();
    //let old_exec: Vec<_> = old_exec.split_whitespace().collect();
    let new_executable_path = "C:\\Program Files\\Mullvad VPN\\resources\\mullvad-daemon.exe";
    let new_args = String::from("--run-as-service -v");
    let mut new_args: Vec<_> = new_args.split_whitespace().collect();
    for arg in new_args.iter_mut() {
        if arg == &"-v" {
            *arg = verbosity;
        }
    }

    log::error!("{:?}", config.executable_path);
    log::error!("{:?}", new_executable_path);
    log::error!("{:?}", new_args);
    //let new_executable_path = config.executable_path;
    // Update the service binary arguments
    let updated_config = ServiceInfo {
        name: config.display_name.clone(),
        display_name: config.display_name.clone(),
        service_type: config.service_type,
        start_type: config.start_type,
        error_control: config.error_control,
        executable_path: std::path::PathBuf::from(new_executable_path),
        launch_arguments: vec![OsString::from("--run-as-service"), OsString::from("-vvv")],//new_args
            //.into_iter()
            //.map(|osstr| OsString::from(osstr.to_string()))
            //.collect(),
        dependencies: config.dependencies.clone(),
        account_name: config.account_name.clone(),
        account_password: None,
    };

    // Apply the updated configuration
    service.change_config(&updated_config).unwrap();
    log::error!("Changing config");

    // Start the service
    service.start::<String>(&[]).unwrap();
    std::thread::sleep_ms(1000);
    log::error!("Done");

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn set_daemon_log_level(verbosity_level: usize) -> Result<(), test_rpc::Error> {
    // TODO: Not implemented
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn toggle_daemon_service(on: bool) -> Result<(), test_rpc::Error> {
    if on {
        std::process::Command::new("systemctl")
            .args(["start", "mullvad-daemon"])
            .spawn()
            .map_err(|e| test_rpc::Error::Shell(e.to_string()))?;
        std::thread::sleep(std::time::Duration::from_millis(1000));
    } else {
        std::process::Command::new("systemctl")
            .args(["stop", "mullvad-daemon"])
            .spawn()
            .map_err(|e| test_rpc::Error::Shell(e.to_string()))?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn toggle_daemon_service(on: bool) -> Result<(), test_rpc::Error> {
    if on {
        std::process::Command::new("net")
            .args(["start", "mullvadvpn"])
            .spawn()
            .map_err(|e| test_rpc::Error::Shell(e.to_string()))?;
        std::thread::sleep(std::time::Duration::from_millis(1000));
    } else {
        std::process::Command::new("net")
            .args(["stop", "mullvadvpn"])
            .spawn()
            .map_err(|e| test_rpc::Error::Shell(e.to_string()))?;
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn toggle_daemon_service(on: bool) -> Result<(), test_rpc::Error> {
    if on {
        std::process::Command::new("launchctl")
            .args([
                "load",
                "-w",
                "/Library/LaunchDaemons/net.mullvad.daemon.plist",
            ])
            .spawn()
            .map_err(|e| test_rpc::Error::Shell(e.to_string()))?;
        std::thread::sleep(std::time::Duration::from_millis(1000));
    } else {
        std::process::Command::new("launchctl")
            .args(["unload", "-w", "/Library/LaunchDaemons/net.mullvad.daemon.plist"])
            .spawn()
            .map_err(|e| test_rpc::Error::Shell(e.to_string()))?;
    }
    Ok(())
}
