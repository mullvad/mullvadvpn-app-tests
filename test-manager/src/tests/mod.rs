use std::{
    path::Path,
    time::{Duration, SystemTime},
};
use tarpc::context;
use test_rpc::{
    meta,
    mullvad_daemon::ServiceStatus,
    package::{Package, PackageType},
    ServiceClient,
};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "RPC call failed")]
    RpcError(tarpc::client::RpcError),

    #[error(display = "Package action failed")]
    PackageError(&'static str, test_rpc::package::Error),

    #[error(display = "Found running daemon unexpectedly")]
    DaemonAlreadyRunning,

    #[error(display = "Daemon unexpectedly not running")]
    DaemonNotRunning,
}

pub async fn test_clean_app_install(rpc: ServiceClient) -> Result<(), Error> {
    // verify that daemon is not already running
    if rpc
        .mullvad_daemon_get_status(context::current())
        .await
        .map_err(Error::RpcError)?
        != ServiceStatus::NotRunning
    {
        return Err(Error::DaemonAlreadyRunning);
    }

    // install package
    let mut ctx = context::current();
    ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

    rpc.install_app(ctx, get_package_desc(&rpc, "current-app").await?)
        .await
        .map_err(Error::RpcError)?
        .map_err(|err| Error::PackageError("current app", err))?;

    // verify that daemon is running
    if rpc
        .mullvad_daemon_get_status(context::current())
        .await
        .map_err(Error::RpcError)?
        != ServiceStatus::Running
    {
        return Err(Error::DaemonNotRunning);
    }

    Ok(())
}

pub async fn test_app_upgrade(rpc: ServiceClient) -> Result<(), Error> {
    // verify that daemon is not already running
    if rpc
        .mullvad_daemon_get_status(context::current())
        .await
        .map_err(Error::RpcError)?
        != ServiceStatus::NotRunning
    {
        return Err(Error::DaemonAlreadyRunning);
    }

    // install old package
    let mut ctx = context::current();
    ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

    rpc.install_app(ctx, get_package_desc(&rpc, "previous-app").await?)
        .await
        .map_err(Error::RpcError)?
        .map_err(|error| Error::PackageError("previous app", error))?;

    // verify that daemon is running
    if rpc
        .mullvad_daemon_get_status(context::current())
        .await
        .map_err(Error::RpcError)?
        != ServiceStatus::Running
    {
        return Err(Error::DaemonNotRunning);
    }

    // give it some time to start
    tokio::time::sleep(Duration::from_secs(3)).await;

    // install new package
    let mut ctx = context::current();
    ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

    rpc.install_app(ctx, get_package_desc(&rpc, "current-app").await?)
        .await
        .map_err(Error::RpcError)?
        .map_err(|error| Error::PackageError("current app", error))?;

    // verify that daemon is running
    if rpc
        .mullvad_daemon_get_status(context::current())
        .await
        .map_err(Error::RpcError)?
        != ServiceStatus::Running
    {
        return Err(Error::DaemonNotRunning);
    }

    // TODO: Verify that all is well

    Ok(())
}

async fn get_package_desc(rpc: &ServiceClient, name: &str) -> Result<Package, Error> {
    match rpc
        .get_os(context::current())
        .await
        .map_err(Error::RpcError)?
    {
        meta::Os::Linux => Ok(Package {
            r#type: PackageType::Dpkg,
            path: Path::new(&format!("/opt/testing/{}.deb", name)).to_path_buf(),
        }),
        meta::Os::Windows => Ok(Package {
            r#type: PackageType::NsisExe,
            path: Path::new(&format!(r"E:\{}.exe", name)).to_path_buf(),
        }),
        _ => unimplemented!(),
    }
}
