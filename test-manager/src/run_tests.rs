use crate::{logging::run_test, mullvad_daemon, tests, vm};
use anyhow::{Context, Result};
use std::time::Duration;
use test_rpc::ServiceClient;

const BAUD: u32 = 115200;

pub async fn run(
    config: tests::config::TestConfig,
    instance: &Box<dyn vm::VmInstance>,
    test_filters: &[String],
) -> Result<()> {
    log::trace!("Setting test constants");
    tests::config::TEST_CONFIG.init(config);

    let pty_path = instance.get_pty();

    log::info!("Connecting to {pty_path}");

    let serial_stream =
        tokio_serial::SerialStream::open(&tokio_serial::new(pty_path, BAUD)).unwrap();
    let (runner_transport, mullvad_daemon_transport, mut connection_handle, completion_handle) =
        test_rpc::transport::create_client_transports(serial_stream).await?;

    connection_handle.wait_for_server().await?;

    log::info!("Running client");

    let client = ServiceClient::new(connection_handle.clone(), runner_transport);
    let mullvad_client =
        mullvad_daemon::new_rpc_client(connection_handle, mullvad_daemon_transport).await;

    let mut tests: Vec<_> = inventory::iter::<tests::TestMetadata>().collect();
    tests.sort_by_key(|test| test.priority.unwrap_or(0));

    if !test_filters.is_empty() {
        tests.retain(|test| {
            for command in test_filters {
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
            .context("Failed to run test")?;

        test_result.print();

        final_result = test_result
            .result
            .context("Test panicked")?
            .context("Test failed");
        if final_result.is_err() {
            break;
        }
    }

    // wait for cleanup
    drop(mullvad_client);
    let _ = tokio::time::timeout(Duration::from_secs(5), completion_handle).await;

    final_result
}
