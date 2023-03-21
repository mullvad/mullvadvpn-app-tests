use crate::config::{Config, VmConfig};
use regex::Regex;
use std::{
    io,
    net::IpAddr,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::process::{Child, Command};
use uuid::Uuid;

use super::{logging::forward_logs, util::find_pty, VmInstance};

const LOG_PREFIX: &str = "[tart] ";
const STDERR_LOG_LEVEL: log::Level = log::Level::Error;
const STDOUT_LOG_LEVEL: log::Level = log::Level::Debug;
const OBTAIN_IP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(err_derive::Error, Debug)]
#[error(no_from)]
pub enum Error {
    #[error(display = "Failed to run 'tart clone'")]
    RunClone(#[error(source)] io::Error),
    #[error(display = "'tart clone' failed: {}", _0)]
    CloneFailed(ExitStatus),
    #[error(display = "Failed to run 'tart delete'")]
    RunDelete(#[error(source)] io::Error),
    #[error(display = "'tart delete' failed: {}", _0)]
    DeleteFailed(ExitStatus),
    #[error(display = "Failed to start Tart")]
    StartTart(#[error(source)] io::Error),
    #[error(display = "Tart exited unexpectedly")]
    TartFailed(Option<ExitStatus>),
    #[error(display = "Failed to obtain IP of guest")]
    ObtainIp(#[error(source)] io::Error),
    #[error(display = "'tart ip' output: invalid utf-8")]
    IpOutputInvalidUtf8,
    #[error(display = "Failed to parse output of 'tart ip'")]
    ParseIpOutput,
    #[error(display = "Could not find pty")]
    NoPty,
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct TartInstance {
    pub pty_path: String,
    pub ip_addr: IpAddr,
    child: Child,
    machine_copy: Option<MachineCopy>,
}

#[async_trait::async_trait]
impl VmInstance for TartInstance {
    fn get_pty(&self) -> &str {
        &self.pty_path
    }

    fn get_ip(&self) -> &IpAddr {
        &self.ip_addr
    }

    async fn wait(&mut self) {
        let _ = self.child.wait().await;
        if let Some(machine) = self.machine_copy.take() {
            machine.cleanup().await;
        }
    }
}

pub async fn run(config: &Config, vm_config: &VmConfig) -> Result<TartInstance> {
    // Create a temporary clone of the machine
    let machine_copy = if config.keep_changes {
        MachineCopy::borrow_vm(&vm_config.image_path)
    } else {
        MachineCopy::clone_vm(&vm_config.image_path).await?
    };

    // Start VM
    let mut tart_cmd = Command::new("tart");
    tart_cmd.args(["run", &machine_copy.name, "--serial"]);

    if !vm_config.disks.is_empty() {
        log::warn!("Mounting disks is not yet supported")
    }

    if !config.display {
        tart_cmd.arg("--no-graphics");
    }

    tart_cmd.stdin(Stdio::piped());
    tart_cmd.stdout(Stdio::piped());
    tart_cmd.stderr(Stdio::piped());

    tart_cmd.kill_on_drop(true);

    let mut child = tart_cmd.spawn().map_err(Error::StartTart)?;

    tokio::spawn(forward_logs(
        LOG_PREFIX,
        child.stderr.take().unwrap(),
        STDERR_LOG_LEVEL,
    ));

    // find pty in stdout
    // match: Successfully open pty /dev/ttys001
    let re = Regex::new(r"Successfully open pty ([/a-zA-Z0-9]+)$").unwrap();
    let pty_path = find_pty(re, &mut child, STDOUT_LOG_LEVEL, LOG_PREFIX)
        .await
        .map_err(|_error| {
            if let Ok(Some(status)) = child.try_wait() {
                return Error::TartFailed(Some(status));
            }
            Error::NoPty
        })?;

    tokio::spawn(forward_logs(
        LOG_PREFIX,
        child.stdout.take().unwrap(),
        STDOUT_LOG_LEVEL,
    ));

    // Get IP address of VM
    log::debug!("Waiting for IP address");

    let mut tart_cmd = Command::new("tart");
    tart_cmd.args([
        "ip",
        &machine_copy.name,
        "--wait",
        &format!("{}", OBTAIN_IP_TIMEOUT.as_secs()),
    ]);
    let output = tart_cmd.output().await.map_err(Error::ObtainIp)?;
    let ip_addr = std::str::from_utf8(&output.stdout)
        .map_err(|_err| Error::IpOutputInvalidUtf8)?
        .trim()
        .parse()
        .map_err(|_err| Error::ParseIpOutput)?;

    log::debug!("Guest IP: {ip_addr}");

    Ok(TartInstance {
        child,
        pty_path,
        ip_addr,
        machine_copy: Some(machine_copy),
    })
}

/// Handle for a transient or borrowed Tart VM.
/// TODO: Prune VMs we fail to delete them somehow.
pub struct MachineCopy {
    name: String,
    should_destroy: bool,
}

impl MachineCopy {
    /// Use an existing VM and save all changes to it.
    pub fn borrow_vm(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            should_destroy: false,
        }
    }

    /// Clone an existing VM and destroy changes when self is dropped.
    pub async fn clone_vm(name: &str) -> Result<Self> {
        let clone_name = format!("test-{}", Uuid::new_v4().to_string());

        let mut tart_cmd = Command::new("tart");
        tart_cmd.args(["clone", name, &clone_name]);
        let output = tart_cmd.status().await.map_err(Error::RunClone)?;
        if !output.success() {
            return Err(Error::CloneFailed(output));
        }

        Ok(Self {
            name: clone_name,
            should_destroy: true,
        })
    }

    pub async fn cleanup(mut self) {
        let _ = tokio::task::spawn_blocking(move || self.try_destroy()).await;
    }

    fn try_destroy(&mut self) {
        if !self.should_destroy {
            return;
        }

        if let Err(error) = self.destroy_inner() {
            log::error!("Failed to destroy Tart clone: {error}");
        } else {
            self.should_destroy = false;
        }
    }

    fn destroy_inner(&mut self) -> Result<()> {
        use std::process::Command;

        let mut tart_cmd = Command::new("tart");
        tart_cmd.args(["delete", &self.name]);
        let output = tart_cmd.status().map_err(Error::RunDelete)?;
        if !output.success() {
            return Err(Error::DeleteFailed(output));
        }

        Ok(())
    }
}

impl Drop for MachineCopy {
    fn drop(&mut self) {
        self.try_destroy();
    }
}
