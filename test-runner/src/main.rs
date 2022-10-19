use futures::{pin_mut, SinkExt, StreamExt};
use logging::LOGGER;
use tarpc::context;
use tarpc::server::Channel;
use test_rpc::{
    meta,
    mullvad_daemon::SOCKET_PATH,
    package::{InstallResult, Package},
    transport::GrpcForwarder,
    Service,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::codec::{Decoder, LengthDelimitedCodec};

mod logging;
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

    let serial_stream = tokio_serial::SerialStream::open(&tokio_serial::new(path, BAUD)).unwrap();
    let (runner_transport, mullvad_daemon_transport, _completion_handle) =
        test_rpc::transport::create_server_transports(serial_stream);

    log::info!("Running server");

    tokio::spawn(foward_to_mullvad_daemon_interface(mullvad_daemon_transport));

    let server = tarpc::server::BaseChannel::with_defaults(runner_transport);
    server.execute(TestServer(()).serve()).await;

    Ok(())
}

/// Forward data between the test manager and Mullvad management interface socket
async fn foward_to_mullvad_daemon_interface(proxy_transport: GrpcForwarder) {
    const IPC_READ_BUF_SIZE: usize = 16 * 1024;

    let mut srv_read_buf = [0u8; IPC_READ_BUF_SIZE];
    let mut proxy_transport = LengthDelimitedCodec::new().framed(proxy_transport);

    loop {
        // Wait for input from the test manager before connecting to the UDS or named pipe.
        // Connect at the last moment since the daemon may not even be running when the
        // test runner first starts.
        let first_message = match proxy_transport.next().await {
            Some(Ok(bytes)) => {
                if bytes.len() == 0 {
                    log::debug!("ignoring EOF from client");
                    continue;
                }
                bytes
            }
            Some(Err(error)) => {
                log::error!("daemon client channel error: {error}");
                break;
            }
            None => break,
        };

        log::info!("mullvad daemon: connecting");

        let mut daemon_socket_endpoint =
            match parity_tokio_ipc::Endpoint::connect(SOCKET_PATH).await {
                Ok(uds_endpoint) => uds_endpoint,
                Err(error) => {
                    log::error!("mullvad daemon: failed to connect: {error}");
                    // send EOF
                    let _ = proxy_transport.send(bytes::Bytes::new());
                    continue;
                }
            };

        log::info!("mullvad daemon: connected");

        if let Err(error) = daemon_socket_endpoint.write_all(&first_message).await {
            log::error!("writing to uds failed: {error}");
            continue;
        }

        loop {
            let srv_read = daemon_socket_endpoint.read(&mut srv_read_buf);
            pin_mut!(srv_read);

            match futures::future::select(srv_read, proxy_transport.next()).await {
                futures::future::Either::Left((read, _)) => match read {
                    Ok(num_bytes) => {
                        if num_bytes == 0 {
                            log::debug!("uds EOF; restarting server");
                            break;
                        }
                        if let Err(error) = proxy_transport
                            .send(srv_read_buf[..num_bytes].to_vec().into())
                            .await
                        {
                            log::error!("writing to client channel failed: {error}");
                            break;
                        }
                    }
                    Err(error) => {
                        log::error!("reading from uds failed: {error}");
                        break;
                    }
                },
                futures::future::Either::Right((read, _)) => match read {
                    Some(Ok(bytes)) => {
                        if bytes.len() == 0 {
                            log::debug!("management interface EOF; restarting server");
                            break;
                        }
                        if let Err(error) = daemon_socket_endpoint.write_all(&bytes).await {
                            log::error!("writing to uds failed: {error}");
                            break;
                        }
                    }
                    Some(Err(error)) => {
                        log::error!("daemon client channel error: {error}");
                        break;
                    }
                    None => break,
                },
            }
        }

        log::info!("mullvad daemon: disconnected");
    }
}
