use server::{meta, mullvad_daemon, package, TestServer};
use tarpc::server::Channel;
use tokio_util::codec::{Decoder, LengthDelimitedCodec};

const BAUD: u32 = 9600;

mod client;
mod server;

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "Test failed")]
    ClientError(#[error(source)] client::tests::Error),

    #[error(display = "Unknown RPC")]
    UnknownRpc,
}

#[tarpc::service]
pub trait Service {
    /// Install app package.
    async fn install_app(package_path: package::Package)
        -> package::Result<package::InstallResult>;

    /// Return status of the system service.
    async fn mullvad_daemon_get_status() -> mullvad_daemon::ServiceStatus;

    //async fn harvest_logs()

    /// Return the OS of the guest.
    async fn get_os() -> meta::Os;

    /// Connect to the VPN.
    async fn mullvad_daemon_connect() -> mullvad_daemon::Result<()>;
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
        server.execute(TestServer(()).serve()).await;
    } else {
        println!("Running client");
        let transport =
            tarpc::serde_transport::new(framed, tokio_serde::formats::Bincode::default());
        let client = ServiceClient::new(tarpc::client::Config::default(), transport).spawn();

        match action.as_ref().map(String::as_str) {
            Some("clean-app-install") => client::tests::test_clean_app_install(client)
                .await
                .map_err(Error::ClientError)?,
            Some("upgrade-app") => client::tests::test_app_upgrade(client)
                .await
                .map_err(Error::ClientError)?,
            _ => return Err(Error::UnknownRpc),
        }
    }

    Ok(())
}
