use tarpc::{context, server::Channel};
use tokio_util::codec::{Decoder, LengthDelimitedCodec};

const BAUD: u32 = 9600;

mod package;
mod tests;

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "Test failed")]
    ClientError(#[error(source)] tests::Error),

    #[error(display = "Unknown RPC")]
    UnknownRpc,
}

#[tarpc::service]
pub trait Service {
    /// Install app package.
    async fn install_app(package_path: package::Package)
        -> package::Result<package::InstallResult>;

    /// Collect information about the daemon, such as whether it is running.
    /// TODO: Check socket or exe or both? Even more generic?
    //async fn poke_service() -> Result<bool, String>;

    //async fn harvest_logs()

    /// Returns the received string.
    async fn echo(message: String) -> String;
}

#[derive(Clone)]
pub struct EchoServer(());

#[tarpc::server]
impl Service for EchoServer {
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

    async fn echo(self, _: context::Context, message: String) -> String {
        println!("Received a message: {message}");

        format!("Response: {message}")
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut args = std::env::args();
    let _ = args.next();
    let path = args.next().expect("serial/COM path must be provided");

    println!("Connecting to {}", path);

    let conn = tokio_serial::SerialStream::open(&tokio_serial::new(path, BAUD)).unwrap();

    let codec = LengthDelimitedCodec::new();
    let framed = codec.framed(conn);

    let action = args.next();

    if action == Some("serve".to_string()) {
        println!("Running server");

        let transport =
            tarpc::serde_transport::new(framed, tokio_serde::formats::Bincode::default());
        let server = tarpc::server::BaseChannel::with_defaults(transport);
        server.execute(EchoServer(()).serve()).await;
    } else {
        println!("Running client");

        let transport =
            tarpc::serde_transport::new(framed, tokio_serde::formats::Bincode::default());
        let client = ServiceClient::new(tarpc::client::Config::default(), transport).spawn();

        match action.as_ref().map(String::as_str) {
            Some("clean-app-install") => tests::test_clean_app_install(client)
                .await
                .map_err(Error::ClientError)?,
            _ => return Err(Error::UnknownRpc),
        }
    }

    Ok(())
}
