use crate::config::{OsType, PackageType, Provisioner, VmConfig};
use crate::vm::ssh::SSHSession;
use anyhow::{Context, Result};
use std::fmt;

#[derive(Debug)]
pub enum Update {
    Success(Vec<String>),
    Nothing,
}

pub async fn packages(config: &VmConfig, instance: &dyn super::VmInstance) -> Result<Update> {
    if let Provisioner::Noop = config.provisioner {
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

    // Authenticate to the VM via SSH.
    log::info!("retrieving SSH credentials");
    let (user, password) = config.get_ssh_options().context("missing SSH config")?;
    let guest_ip = *instance.get_ip();
    let ssh = SSHSession::new(user, password, guest_ip).await?;

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
            let mut result = vec![];
            for command in commands {
                log::info!("Running {command} in guest");
                let output = ssh.exec(command).await?;
                result.push(output);
            }
            Update::Success(result)
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
