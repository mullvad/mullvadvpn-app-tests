use tarpc::context;
use tarpc::server::Channel;
use test_rpc::{
    meta,
    package::{InstallResult, Package},
    Service,
};
use tokio_util::codec::{Decoder, LengthDelimitedCodec};

mod mullvad_daemon;
mod package;

#[derive(Clone)]
pub struct TestServer(pub ());

#[tarpc::server]
impl Service for TestServer {
    async fn install_app(
        self,
        _: context::Context,
        package: Package,
    ) -> test_rpc::package::Result<InstallResult> {
        println!("Running installer");

        let result = package::install_package(package).await?;

        println!("Done");

        Ok(result)
    }

    async fn get_os(self, _: context::Context) -> meta::Os {
        meta::CURRENT_OS
    }

    async fn mullvad_daemon_get_status(
        self,
        _: context::Context,
    ) -> test_rpc::mullvad_daemon::ServiceStatus {
        mullvad_daemon::get_status()
    }

    async fn mullvad_daemon_connect(
        self,
        _: context::Context,
    ) -> test_rpc::mullvad_daemon::Result<()> {
        mullvad_daemon::connect().await
    }
}

const BAUD: u32 = 115200;

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "Unknown RPC")]
    UnknownRpc,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::init();

    let mut args = std::env::args();
    let _ = args.next();
    let path = args.next().expect("serial/COM path must be provided");

    println!("Connecting to {}", path);

    let conn = tokio_serial::SerialStream::open(&tokio_serial::new(path, BAUD)).unwrap();

    let codec = LengthDelimitedCodec::new();
    let framed = codec.framed(conn);

    println!("Running server");

    let transport = tarpc::serde_transport::new(framed, tokio_serde::formats::Bincode::default());
    let server = tarpc::server::BaseChannel::with_defaults(transport);
    server.execute(TestServer(()).serve()).await;

    Ok(())
}
