use std::path::Path;

use tarpc::context;

use crate::{ServiceClient, package};

const APP_CURRENT_VERSION_PATH: &str = "/opt/testing/current-app.deb";
const APP_PREVIOUS_VERSION_PATH: &str = "/opt/testing/previous-app.deb";

pub enum Error {
    RpcError(tarpc::client::RpcError),
    PackageError(package::Error),
}

pub async fn test_clean_app_install(rpc: ServiceClient) -> Result<(), Error> {
    // verify that daemon is not already running

    // install package
    rpc.install_app(context::current(), package::Package {
        r#type: package::PackageType::Dpkg,
        // TODO: pass in path somehow
        path: Path::new(APP_CURRENT_VERSION_PATH).to_path_buf(),
    })
    .await
    .map_err(Error::RpcError)?
    .map_err(|err| Error::PackageError(err))?;

    // verify that daemon is running

    Ok(())
}
