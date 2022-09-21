use std::path::Path;

use tarpc::context;

use crate::{server::{app, package}, ServiceClient};

const APP_CURRENT_VERSION_PATH: &str = "/opt/testing/current-app.deb";
const APP_PREVIOUS_VERSION_PATH: &str = "/opt/testing/previous-app.deb";

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "RPC call failed")]
    RpcError(tarpc::client::RpcError),

    #[error(display = "Package action failed")]
    PackageError(package::Error),

    #[error(display = "Found running daemon unexpectedly")]
    DaemonAlreadyRunning,

    #[error(display = "Daemon unexpectedly not running")]
    DaemonNotRunning,
}

pub async fn test_clean_app_install(rpc: ServiceClient) -> Result<(), Error> {
    // verify that daemon is not already running
    if rpc
        .poke_service(context::current())
        .await
        .map_err(Error::RpcError)?
        != app::ServiceStatus::NotRunning
    {
        return Err(Error::DaemonAlreadyRunning);
    }

    // install package
    rpc.install_app(
        context::current(),
        package::Package {
            r#type: package::PackageType::Dpkg,
            // TODO: pass in path somehow
            path: Path::new(APP_CURRENT_VERSION_PATH).to_path_buf(),
        },
    )
    .await
    .map_err(Error::RpcError)?
    .map_err(|err| Error::PackageError(err))?;

    // verify that daemon is running
    if rpc
        .poke_service(context::current())
        .await
        .map_err(Error::RpcError)?
        != app::ServiceStatus::Running
    {
        return Err(Error::DaemonNotRunning);
    }

    Ok(())
}
