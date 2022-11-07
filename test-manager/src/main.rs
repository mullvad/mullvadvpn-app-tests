mod logging;
mod mullvad_daemon;
mod network_monitor;
mod tests;
use logging::run_test;
use std::time::Duration;
use test_rpc::ServiceClient;

const BAUD: u32 = 115200;

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "Test failed")]
    ClientError(#[error(source)] tests::Error),

    #[error(display = "Unknown RPC")]
    UnknownRpc,

    #[error(display = "RPC error")]
    RpcError(#[error(source)] test_rpc::Error),
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
        test_rpc::transport::create_client_transports(serial_stream).await?;

    println!("Running client");

    let client = ServiceClient::new(tarpc::client::Config::default(), runner_transport).spawn();
    let mullvad_client = mullvad_daemon::new_rpc_client(mullvad_daemon_transport).await;

    let mut tests = tests::manager_tests::ManagerTests::new().tests;
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

    for test in tests {
        let mclient = mullvad_client.client().await;

        log::info!("Running {}", test.name);
        let test_result = run_test(client.clone(), mclient, test.func, test.name)
            .await
            .map_err(Error::ClientError)?;
        test_result.print();
        if !matches!(test_result.result, Ok(Ok(_))) {
            break;
        }
    }

    // wait for cleanup
    drop(mullvad_client);
    let _ = tokio::time::timeout(Duration::from_secs(5), completion_handle).await;

    Ok(())
}
