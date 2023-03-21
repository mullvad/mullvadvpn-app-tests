#![cfg(target_os = "linux")]

use tokio::process::Command;

/// Re-launch self with rootlesskit if we're not root.
/// Allows for rootless and containerized networking.
pub async fn relaunch_with_rootlesskit() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }

    let mut cmd = Command::new("rootlesskit");
    cmd.args([
        "--net",
        "slirp4netns",
        "--disable-host-loopback",
        "--copy-up=/etc",
    ]);
    cmd.args(std::env::args());

    let status = cmd.status().await.unwrap();

    std::process::exit(status.code().unwrap_or(1));
}
