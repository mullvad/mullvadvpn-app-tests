#[cfg(target_os = "windows")]
use std::io;

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

    let verbosity = if verbosity_level == 0{
        ""
    } else if verbosity_level == 1 {
        " -v"
    } else if verbosity_level == 2 {
        " -vv"
    } else {
        " -vvv"
    };
    let systemd_service_file_content = format!(r#"# Systemd service unit file for the Mullvad VPN daemon
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
WantedBy=multi-user.target"#);

    std::fs::write(
        SYSTEMD_SERVICE_FILE,
        systemd_service_file_content
    )
    .unwrap();

    std::process::Command::new("sudo").args(["systemctl", "daemon-reload"]).spawn().unwrap();
    std::process::Command::new("sudo").args(["systemctl", "restart", "mullvad-daemon"]).spawn().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(2000));
    Ok(())
}
