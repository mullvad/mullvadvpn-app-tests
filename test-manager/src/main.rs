mod tests;
mod logging;
use logging::print_log_on_error;

use test_rpc::ServiceClient;
use tokio_util::codec::{Decoder, LengthDelimitedCodec};

const BAUD: u32 = 115200;

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "Test failed")]
    ClientError(#[error(source)] tests::Error),

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

    println!("Running client");
    let transport = tarpc::serde_transport::new(framed, tokio_serde::formats::Json::default());
    let client = ServiceClient::new(tarpc::client::Config::default(), transport).spawn();

    match args.next().as_deref() {
        Some("clean-app-install") => print_log_on_error(client, tests::test_clean_app_install, "clean_app_install")
            .await
            .map_err(Error::ClientError)?,
        Some("upgrade-app") => print_log_on_error(client, tests::test_app_upgrade, "test_app_upgrade")
            .await
            .map_err(Error::ClientError)?,
        _ => return Err(Error::UnknownRpc),
    }

    Ok(())
}
