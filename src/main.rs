use tarpc::{context, server::Channel};
use tokio_util::codec::{Decoder, LengthDelimitedCodec};

const BAUD: u32 = 9600;

mod package;

#[tarpc::service]
trait Service {
    /// Install a given package
    async fn install_app(package_path: package::Package) -> package::Result<package::InstallResult>;

    /// Returns the received string.
    async fn echo(message: String) -> String;
}

#[derive(Clone)]
struct EchoServer(());

#[tarpc::server]
impl Service for EchoServer {
    async fn install_app(self, _: context::Context, package: package::Package) -> package::Result<package::InstallResult> {


        println!("Running installer");

        package::install_package(package).await
    }

    async fn echo(self, _: context::Context, message: String) -> String {
        println!("Received a message: {message}");

        format!("Response: {message}")
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args();
    let _ = args.next();
    let path = args.next().expect("serial/COM path must be provided");

    println!("Connecting to {}", path);

    let conn = tokio_serial::SerialStream::open(&tokio_serial::new(path, BAUD)).unwrap();

    let codec = LengthDelimitedCodec::new();
    let framed = codec.framed(conn);

    if args.next() == Some("serve".to_string()) {
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

        let response = client
            .echo(context::current(), "Hello, World!".to_string())
            .await?;

        println!("Served replied: {response}");
    }

    Ok(())
}
