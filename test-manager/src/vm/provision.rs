use crate::config::{Provisioner, VmConfig};

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "artifacts_dir must be set to a mountpoint")]
    MissingArtifactsDir,
}

pub type Result<T> = std::result::Result<T, Error>;

pub async fn provision(
    config: &VmConfig,
    _instance: &Box<dyn super::VmInstance>,
) -> Result<String> {
    match config.provisioner {
        Provisioner::Noop => {
            let dir = config
                .artifacts_dir
                .as_ref()
                .ok_or(Error::MissingArtifactsDir)?;
            Ok(dir.clone())
        }
    }
}
