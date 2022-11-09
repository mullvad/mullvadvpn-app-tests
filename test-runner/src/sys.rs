use std::io;


#[cfg(target_os = "macos")]
pub fn reboot() -> Result<(), test_rpc::Error> {
    unimplemented!("not implemented")
}

#[cfg(target_os = "windows")]
pub fn reboot() -> Result<(), test_rpc::Error> {
    use std::ptr;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Shutdown::{
        InitiateShutdownW,
        SHUTDOWN_HYBRID,
        SHUTDOWN_FORCE_SELF,
        SHUTDOWN_RESTART,
    };

    let shutdown_result = unsafe {
        InitiateShutdownW(
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            SHUTDOWN_RESTART | SHUTDOWN_FORCE_SELF | SHUTDOWN_HYBRID,
            0,
        )
    };

    if shutdown_result == ERROR_SUCCESS {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    log::error!("Failed to restart test machine: {error}");
    Err(test_rpc::Error::Syscall)
}

#[cfg(target_os = "linux")]
pub fn reboot() -> Result<(), test_rpc::Error> {
    log::debug!("Rebooting system");

    match unsafe { libc::fork() } {
        0 => {
            // child process
            unsafe { libc::reboot(libc::LINUX_REBOOT_CMD_RESTART) };
            unreachable!("reboot failed")
        }
        -1 => {
            log::error!("fork() returned an error: {}", io::Error::last_os_error());
            Err(test_rpc::Error::Syscall)
        }
        // parent process
        _ => Ok(()),
    }
}
