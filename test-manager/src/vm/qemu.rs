use crate::{
    config::{Config, VmConfig},
    vm::logging::forward_logs,
};
use regex::Regex;
use std::{
    io,
    net::IpAddr,
    ops::Deref,
    process::{ExitStatus, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    time::timeout,
};

use super::{network, VmInstance};

const LOG_PREFIX: &str = "[qemu] ";
const STDERR_LOG_LEVEL: log::Level = log::Level::Error;
const STDOUT_LOG_LEVEL: log::Level = log::Level::Debug;
const OBTAIN_PTY_TIMEOUT: Duration = Duration::from_secs(5);
const OBTAIN_IP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "Failed to set up network")]
    Network(network::Error),
    #[error(display = "Failed to start QEMU")]
    StartQemu(io::Error),
    #[error(display = "QEMU exited unexpectedly")]
    QemuFailed(Option<ExitStatus>),
    #[error(display = "Could not find pty")]
    NoPty,
    #[error(display = "Could not find IP address of guest")]
    NoIpAddr,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct QemuInstance(Arc<QemuInstanceInner>);

impl Deref for QemuInstance {
    type Target = QemuInstanceInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct QemuInstanceInner {
    pub pty_path: String,
    pub ip_addr: IpAddr,
    child: Child,
    _network_handle: network::NetworkHandle,
}

#[async_trait::async_trait]
impl VmInstance for QemuInstance {
    fn get_pty(&self) -> &str {
        &self.pty_path
    }

    fn get_ip(&self) -> &IpAddr {
        &self.ip_addr
    }

    async fn wait(&mut self) {
        let inner = Arc::get_mut(&mut self.0).unwrap();
        let _ = inner.child.wait().await;
    }
}

pub async fn run(config: &Config, vm_config: &VmConfig) -> Result<QemuInstance> {
    let mut network_handle = network::create().await.map_err(Error::Network)?;

    let mut qemu_cmd = Command::new("qemu-system-x86_64");
    qemu_cmd.args([
        "-cpu",
        "host",
        "-accel",
        "kvm",
        "-m",
        "4096",
        "-smp",
        "2",
        "-drive",
        &format!("file={}", vm_config.image_path),
        "-device",
        "virtio-serial-pci",
        "-serial",
        "pty",
        // attach to TAP interface
        "-nic",
        &format!("tap,ifname={},script=no,downscript=no", network::TAP_NAME),
        "-device",
        "nec-usb-xhci,id=xhci",
    ]);

    if !config.keep_changes {
        qemu_cmd.arg("-snapshot");
    }

    if !config.display {
        qemu_cmd.args(["-display", "none"]);
    }

    for (i, disk) in vm_config.disks.iter().enumerate() {
        qemu_cmd.args([
            "-drive",
            &format!("if=none,id=disk{i},file={disk}"),
            "-device",
            &format!("usb-storage,drive=disk{i},bus=xhci.0"),
        ]);
    }

    qemu_cmd.stdin(Stdio::piped());
    qemu_cmd.stdout(Stdio::piped());
    qemu_cmd.stderr(Stdio::piped());

    qemu_cmd.kill_on_drop(true);

    let mut child = qemu_cmd.spawn().map_err(Error::StartQemu)?;

    tokio::spawn(forward_logs(
        LOG_PREFIX,
        child.stderr.take().unwrap(),
        STDERR_LOG_LEVEL,
    ));

    let pty_path = find_pty(&mut child).await.map_err(|error| {
        if let Ok(status) = child.try_wait() {
            return Error::QemuFailed(status);
        }
        error
    })?;

    tokio::spawn(forward_logs(
        LOG_PREFIX,
        child.stdout.take().unwrap(),
        STDOUT_LOG_LEVEL,
    ));

    log::debug!("Waiting for IP address");
    let ip_addr = timeout(OBTAIN_IP_TIMEOUT, network_handle.first_dhcp_ack())
        .await
        .map_err(|_| Error::NoIpAddr)?
        .ok_or(Error::NoIpAddr)?;
    log::debug!("Guest IP: {ip_addr}");

    Ok(QemuInstance(Arc::new(QemuInstanceInner {
        pty_path,
        ip_addr,
        child,
        _network_handle: network_handle,
    })))
}

async fn find_pty(process: &mut tokio::process::Child) -> Result<String> {
    // match: char device redirected to /dev/pts/0 (label serial0)
    let re = Regex::new(r"char device redirected to ([/a-zA-Z0-9]+) \(").unwrap();

    let stdout = process.stdout.take().unwrap();
    let stdout_reader = BufReader::new(stdout);

    let (pty_path, reader) = timeout(OBTAIN_PTY_TIMEOUT, async {
        let mut lines = stdout_reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            log::log!(STDOUT_LOG_LEVEL, "{LOG_PREFIX}{line}");

            if let Some(path) = re.captures(&line).and_then(|cap| cap.get(1)) {
                return Ok((path.as_str().to_owned(), lines.into_inner()));
            }
        }

        Err(Error::NoPty)
    })
    .await
    .map_err(|_| Error::NoPty)??;

    process.stdout.replace(reader.into_inner());

    Ok(pty_path)
}
