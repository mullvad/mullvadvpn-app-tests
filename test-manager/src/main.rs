mod config;
mod logging;
mod mullvad_daemon;
mod network_monitor;
mod tests;
use std::time::Duration;

use logging::run_test;
use test_rpc::ServiceClient;

const BAUD: u32 = 115200;

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "Test failed")]
    ClientError(#[error(source)] tests::Error),

    #[error(display = "Test panicked")]
    TestPanic(Box<dyn std::any::Any + Send>),

    #[error(display = "RPC error")]
    RpcError(#[error(source)] test_rpc::Error),
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    init_logger();

    let mut args = std::env::args();
    let _ = args.next();
    let path = args.next().expect("serial/COM path must be provided");

    log::info!("Connecting to {}", path);

    let serial_stream = tokio_serial::SerialStream::open(&tokio_serial::new(&path, BAUD)).unwrap();
    let (runner_transport, mullvad_daemon_transport, mut connection_handle, completion_handle) =
        test_rpc::transport::create_client_transports(serial_stream).await?;

    connection_handle.wait_for_server().await?;

    log::info!("Running client");

    let client = ServiceClient::new(connection_handle.clone(), runner_transport);
    let mullvad_client = mullvad_daemon::new_rpc_client(connection_handle, mullvad_daemon_transport).await;

    let mut tests: Vec<_> = inventory::iter::<tests::TestMetadata>().collect();
    tests.sort_by_key(|test| test.priority.unwrap_or(0));

    let test_args: Vec<String> = args.into_iter().collect();
    if !test_args.is_empty() {
        tests.retain(|test| {
            for command in &test_args {
                let command = command.to_lowercase();
                if test.command.to_lowercase().contains(&command) {
                    return true;
                }
            }
            false
        });
    }

    let mut final_result = Ok(());

    for test in tests {
        let mclient = mullvad_client.as_type(test.mullvad_client_version).await;

        log::info!("Running {}", test.name);
        let test_result = run_test(client.clone(), mclient, &test.func, test.name)
            .await
            .map_err(Error::ClientError)?;
        test_result.print();

        final_result = test_result
            .result
            .map_err(Error::TestPanic)?
            .map_err(Error::ClientError);
        if final_result.is_err() {
            break;
        }
    }

    // wait for cleanup
    drop(mullvad_client);
    let _ = tokio::time::timeout(Duration::from_secs(5), completion_handle).await;

    final_result
}

fn init_logger() {
    let mut logger = env_logger::Builder::new();
    logger.filter_module("h2", log::LevelFilter::Info);
    logger.filter_module("tower", log::LevelFilter::Info);
    logger.filter_module("hyper", log::LevelFilter::Info);
    logger.filter_module("rustls", log::LevelFilter::Info);
    logger.filter_level(log::LevelFilter::Debug);
    logger.parse_env(env_logger::DEFAULT_FILTER_ENV);
    logger.init();
}
