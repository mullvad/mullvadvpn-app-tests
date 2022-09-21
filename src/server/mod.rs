use crate::Service;
use tarpc::context;

pub mod app;
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

    async fn poke_service(self, _: context::Context) -> app::ServiceStatus {
        app::poke_service()
    }

    async fn echo(self, _: context::Context, message: String) -> String {
        println!("Received a message: {message}");

        format!("Response: {message}")
    }
}
