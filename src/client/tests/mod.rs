use std::{path::Path, time::Duration};

use tarpc::context;

use crate::{
    server::{app, meta, package},
    ServiceClient,
};

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "RPC call failed")]
    RpcError(tarpc::client::RpcError),

    #[error(display = "Package action failed")]
    PackageError(&'static str, package::Error),

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
        get_package_desc(&rpc, "current-app").await?,
    )
    .await
    .map_err(Error::RpcError)?
    .map_err(|err| Error::PackageError("current app", err))?;

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

pub async fn test_app_upgrade(rpc: ServiceClient) -> Result<(), Error> {
    // verify that daemon is not already running
    if rpc
        .poke_service(context::current())
        .await
        .map_err(Error::RpcError)?
        != app::ServiceStatus::NotRunning
    {
        return Err(Error::DaemonAlreadyRunning);
    }

    // install old package
    rpc.install_app(
        context::current(),
        get_package_desc(&rpc, "previous-app").await?,
    )
    .await
    .map_err(Error::RpcError)?
    .map_err(|error| Error::PackageError("previous app", error))?;

    // verify that daemon is running
    if rpc
        .poke_service(context::current())
        .await
        .map_err(Error::RpcError)?
        != app::ServiceStatus::Running
    {
        return Err(Error::DaemonNotRunning);
    }

    // give it some time to start
    tokio::time::sleep(Duration::from_secs(3)).await;

    // install new package
    rpc.install_app(
        context::current(),
        get_package_desc(&rpc, "current-app").await?,
    )
    .await
    .map_err(Error::RpcError)?
    .map_err(|error| Error::PackageError("current app", error))?;

    // verify that daemon is running
    if rpc
        .poke_service(context::current())
        .await
        .map_err(Error::RpcError)?
        != app::ServiceStatus::Running
    {
        return Err(Error::DaemonNotRunning);
    }

    // TODO: Verify that all is well

    Ok(())
}

async fn get_package_desc(rpc: &ServiceClient, name: &str) -> Result<package::Package, Error> {
    match rpc
        .get_os(context::current())
        .await
        .map_err(Error::RpcError)?
    {
        meta::Os::Linux => Ok(package::Package {
            r#type: package::PackageType::Dpkg,
            path: Path::new(&format!("/opt/testing/{}.deb", name)).to_path_buf(),
        }),
        meta::Os::Windows => Ok(package::Package {
            r#type: package::PackageType::NsisExe,
            path: Path::new(&format!(r"E:\{}.exe", name)).to_path_buf(),
        }),
        _ => unimplemented!(),
    }
}
