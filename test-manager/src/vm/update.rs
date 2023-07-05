use crate::config::{OsType, PackageType, Provisioner, VmConfig};
use crate::vm::ssh::{SSHCredentials, SSHSession};
use anyhow::{Context, Result};
use std::fmt;

#[derive(Debug)]
pub enum Update {
    Success(Vec<String>),
    Nothing,
}

pub async fn packages(config: &VmConfig, instance: &dyn super::VmInstance) -> Result<Update> {
    if Provisioner::Noop == config.provisioner {
        return Ok(Update::Nothing);
    }
    // User SSH session to execute package manager update command.
    // This will of course be dependant on the target platform.
    let commands = match (config.os_type, config.package_type) {
        (OsType::Linux, Some(PackageType::Deb)) => {
            Some(vec!["sudo apt update", "sudo apt -y upgrade"])
        }
        (OsType::Linux, Some(PackageType::Rpm)) => Some(vec!["sudo dnf update"]),
        (OsType::Linux, _) => None,
        (OsType::Macos | OsType::Windows, _) => None,
    };

    log::info!("retrieving SSH credentials");
    let (user, password) = config.get_ssh_options().context("missing SSH config")?;
    let ssh_credentials = SSHCredentials::new(user, password);
    let guest_ip = *instance.get_ip();

    // Issue the update command(s).
    let result = match commands {
        None => {
            log::info!("No update command was found");
            log::debug!(
                "Tried to invoke package update for platform {:?} with package type {:?}",
                config.os_type,
                config.package_type
            );
            Update::Nothing
        }
        Some(commands) => {
            let output = tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
                let ssh = SSHSession::connect(ssh_credentials, guest_ip)?;
                commands
                    .iter()
                    .map(|command| {
                        log::info!("Running {command} in guest");
                        ssh.exec_blocking(command)
                    })
                    .collect()
            })
            .await??;
            Update::Success(output)
        }
    };

    Ok(result)
}

// Pretty-printing for an `Update` action.
impl fmt::Display for Update {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Update::Nothing => write!(formatter, "Nothing was updated"),
            Update::Success(commands) => commands
                .iter()
                .try_for_each(|output| write!(formatter, "{}", output)),
        }
    }
}
