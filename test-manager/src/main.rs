mod logging;
mod mullvad_daemon;
mod network_monitor;
mod tests;

use logging::print_log_on_error;

use test_rpc::ServiceClient;

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
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let mut args = std::env::args();
    let _ = args.next();
    let path = args.next().expect("serial/COM path must be provided");

    println!("Connecting to {}", path);

    let serial_stream = tokio_serial::SerialStream::open(&tokio_serial::new(&path, BAUD)).unwrap();
    let (runner_transport, mullvad_daemon_transport, completion_handle) =
        test_rpc::transport::create_client_transports(serial_stream);

    println!("Running client");

    let client = ServiceClient::new(tarpc::client::Config::default(), runner_transport).spawn();

    match args.next().as_deref() {
        Some("clean-app-install") => {
            print_log_on_error(client, tests::test_clean_app_install, "clean_app_install")
                .await
                .map_err(Error::ClientError)?
        }
        Some("upgrade-app") => {
            print_log_on_error(client, tests::test_app_upgrade, "test_app_upgrade")
                .await
                .map_err(Error::ClientError)?
        }
        Some("test-grpc") => {
            let mut mullvad_client = mullvad_daemon::new_rpc_client(mullvad_daemon_transport).await;
            log::info!(
                "Tunnel state here: {:?}",
                mullvad_client.get_tunnel_state(()).await.unwrap()
            );

            // wait for cleanup
            drop(mullvad_client);
            let _ = completion_handle.await;
        }
        _ => return Err(Error::UnknownRpc),
    }

    Ok(())
}
