use tarpc::context;
use tarpc::server::Channel;
use test_rpc::{
    meta,
    package::{InstallResult, Package},
    Service,
};
use tokio_util::codec::{Decoder, LengthDelimitedCodec};
use logging::LOGGER;

mod mullvad_daemon;
mod package;
mod logging;

#[derive(Clone)]
pub struct TestServer(pub ());

#[tarpc::server]
impl Service for TestServer {
    async fn install_app(
        self,
        _: context::Context,
        package: Package,
    ) -> test_rpc::package::Result<InstallResult> {
        let result = package::install_package(package).await?;

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

    async fn poll_output(
        self,
        _: context::Context,
    ) -> test_rpc::mullvad_daemon::Result<Vec<test_rpc::logging::Output>> {
        let mut listener = LOGGER.0.lock().await;
        if let Ok(output) = listener.recv().await {
            let mut buffer = vec![output];
            while let Ok(output) = listener.try_recv() {
                buffer.push(output);
            }
            Ok(buffer)
        } else {
            Err(test_rpc::mullvad_daemon::Error::CanNotGetOutput)
        }
    }

    async fn try_poll_output(
        self,
        _: context::Context,
    ) -> test_rpc::mullvad_daemon::Result<Vec<test_rpc::logging::Output>> {
        let mut listener = LOGGER.0.lock().await;
        if let Ok(output) = listener.try_recv() {
            let mut buffer = vec![output];
            while let Ok(output) = listener.try_recv() {
                buffer.push(output);
            }
            Ok(buffer)
        } else {
            Err(test_rpc::mullvad_daemon::Error::CanNotGetOutput)
        }
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
    logging::init_logger().unwrap();

    let mut args = std::env::args();
    let _ = args.next();
    let path = args.next().expect("serial/COM path must be provided");

    log::info!("Connecting to {}", path);

    let conn = tokio_serial::SerialStream::open(&tokio_serial::new(path, BAUD)).unwrap();

    let codec = LengthDelimitedCodec::new();
    let framed = codec.framed(conn);

    log::info!("Running server");

    let transport = tarpc::serde_transport::new(framed, tokio_serde::formats::Json::default());
    let server = tarpc::server::BaseChannel::with_defaults(transport);
    server.execute(TestServer(()).serve()).await;

    Ok(())
}
