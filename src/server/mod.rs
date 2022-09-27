use crate::Service;
use tarpc::context;

pub mod meta;
pub mod mullvad_daemon;
pub mod package;

#[derive(Clone)]
pub struct TestServer(pub ());

#[tarpc::server]
impl Service for TestServer {
    async fn install_app(
        self,
        _: context::Context,
        package: package::Package,
    ) -> package::Result<package::InstallResult> {
        println!("Running installer");

        let result = package::install_package(package).await?;

        println!("Done");

        Ok(result)
    }

    async fn get_mullvad_daemon_status(self, _: context::Context) -> mullvad_daemon::ServiceStatus {
        mullvad_daemon::get_status()
    }

    async fn get_os(self, _: context::Context) -> meta::Os {
        meta::CURRENT_OS
    }
}
