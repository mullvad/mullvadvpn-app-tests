use crate::config::{Provisioner, VmConfig};
use anyhow::{Context, Result};

pub async fn provision(
    config: &VmConfig,
    _instance: &Box<dyn super::VmInstance>,
) -> Result<String> {
    match config.provisioner {
        Provisioner::Noop => {
            let dir = config
                .artifacts_dir
                .as_ref()
                .context("'artifacts_dir' must be set to a mountpoint")?;
            Ok(dir.clone())
        }
    }
}
