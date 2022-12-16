use futures::{pin_mut, SinkExt, StreamExt};
use logging::LOGGER;
use std::{
    net::{IpAddr, SocketAddr},
    path::Path, time::Duration,
};

use tarpc::context;
use tarpc::server::Channel;
use test_rpc::{
    meta,
    mullvad_daemon::{ServiceStatus, SOCKET_PATH},
    package::Package,
    transport::GrpcForwarder,
    AppTrace, Interface, Service,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast::error::TryRecvError;
use tokio_util::codec::{Decoder, LengthDelimitedCodec};

mod app;
mod logging;
mod net;
mod package;

#[derive(Clone)]
pub struct TestServer(pub ());

#[tarpc::server]
impl Service for TestServer {
    async fn install_app(
        self,
        _: context::Context,
        package: Package,
    ) -> test_rpc::package::Result<()> {
        log::debug!("Installing app");

        package::install_package(package).await?;

        log::debug!("Install complete");

        Ok(())
    }

    async fn uninstall_app(self, _: context::Context) -> test_rpc::package::Result<()> {
        log::debug!("Uninstalling app");

        package::uninstall_app().await?;

        log::debug!("Uninstalled app");

        Ok(())
    }

    async fn get_os(self, _: context::Context) -> meta::Os {
        meta::CURRENT_OS
    }

    async fn mullvad_daemon_get_status(
        self,
        _: context::Context,
    ) -> test_rpc::mullvad_daemon::ServiceStatus {
        match Path::new(SOCKET_PATH).exists() {
            true => ServiceStatus::Running,
            false => ServiceStatus::NotRunning,
        }
    }

    async fn find_mullvad_app_traces(
        self,
        _: context::Context,
    ) -> Result<Vec<AppTrace>, test_rpc::Error> {
        app::find_traces()
    }

    async fn send_tcp(
        self,
        _: context::Context,
        bind_addr: SocketAddr,
        destination: SocketAddr,
    ) -> Result<(), ()> {
        net::send_tcp(bind_addr, destination).await
    }

    async fn send_udp(
        self,
        _: context::Context,
        bind_addr: SocketAddr,
        destination: SocketAddr,
    ) -> Result<(), ()> {
        net::send_udp(bind_addr, destination).await
    }

    async fn send_ping(
        self,
        _: context::Context,
        interface: Option<Interface>,
        destination: IpAddr,
    ) -> Result<(), ()> {
        net::send_ping(interface, destination).await
    }

    async fn geoip_lookup(
        self,
        _: context::Context,
    ) -> Result<test_rpc::AmIMullvad, test_rpc::Error> {
        net::geoip_lookup().await
    }

    async fn resolve_hostname(
        self,
        _: context::Context,
        hostname: String,
    ) -> Result<Vec<SocketAddr>, test_rpc::Error> {
        Ok(tokio::net::lookup_host(hostname)
            .await
            .map_err(|error| {
                log::debug!("resolve_hostname failed: {error}");
                test_rpc::Error::DnsResolution
            })?
            .collect())
    }

    async fn get_interface_ip(
        self,
        _: context::Context,
        interface: Interface,
    ) -> Result<IpAddr, test_rpc::Error> {
        net::get_interface_ip(interface)
    }

    async fn poll_output(
        self,
        _: context::Context,
    ) -> test_rpc::logging::Result<Vec<test_rpc::logging::Output>> {
        let mut listener = LOGGER.0.lock().await;
        if let Ok(output) = listener.recv().await {
            let mut buffer = vec![output];
            while let Ok(output) = listener.try_recv() {
                buffer.push(output);
            }
            Ok(buffer)
        } else {
            Err(test_rpc::logging::Error::StandardOutput)
        }
    }

    async fn try_poll_output(
        self,
        _: context::Context,
    ) -> test_rpc::logging::Result<Vec<test_rpc::logging::Output>> {
        let mut listener = LOGGER.0.lock().await;
        match listener.try_recv() {
            Ok(output) => {
                let mut buffer = vec![output];
                while let Ok(output) = listener.try_recv() {
                    buffer.push(output);
                }
                Ok(buffer)
            }
            Err(TryRecvError::Empty) => Ok(Vec::new()),
            Err(_) => Err(test_rpc::logging::Error::StandardOutput),
        }
    }

    async fn get_mullvad_app_logs(self, _: context::Context) -> test_rpc::logging::LogOutput {
        logging::get_mullvad_app_logs().await
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

    loop {
        log::info!("Connecting to {}", path);

        let mut serial_stream =
            tokio_serial::SerialStream::open(&tokio_serial::new(&path, BAUD)).unwrap();
        discard_partial_frames(&mut serial_stream).await;
        let (runner_transport, mullvad_daemon_transport, _completion_handle) =
            test_rpc::transport::create_server_transports(serial_stream);

        log::info!("Running server");

        tokio::spawn(foward_to_mullvad_daemon_interface(mullvad_daemon_transport));

        let server = tarpc::server::BaseChannel::with_defaults(runner_transport);
        server.execute(TestServer(()).serve()).await;

        log::error!("Restarting server since it stopped");
    }
}

// Try to discard partial frames. This actually discards all data, which should be safe since all of it
// should be "ping" frames. If a "ping" is received simultaneously, this may still leave partial data,
// but that is unlikely.
async fn discard_partial_frames(serial_stream: &mut tokio_serial::SerialStream) {
    let sleep = tokio::time::sleep(Duration::from_secs(1));
    let discard_bytes = async {
        while let Ok(bytes_read) = serial_stream.read(&mut [0u8; 2048]).await {
            log::debug!("Discarded {bytes_read} bytes");
        }
    };
    futures::pin_mut!(sleep);
    futures::pin_mut!(discard_bytes);
    futures::future::select(sleep, discard_bytes).await;
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
                if bytes.is_empty() {
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
                        let _ = proxy_transport.send(bytes::Bytes::new()).await;
                        break;
                    }
                },
                futures::future::Either::Right((read, _)) => match read {
                    Some(Ok(bytes)) => {
                        if bytes.is_empty() {
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
