use crate::{logging::run_test, mullvad_daemon, tests, vm};
use anyhow::{Context, Result};
use mullvad_management_interface::ManagementServiceClient;
use std::time::Duration;
use test_rpc::{mullvad_daemon::MullvadClientVersion, ServiceClient};

const BAUD: u32 = 115200;

pub async fn run(
    config: tests::config::TestConfig,
    instance: &Box<dyn vm::VmInstance>,
    test_filters: &[String],
    skip_wait: bool,
) -> Result<()> {
    log::trace!("Setting test constants");
    tests::config::TEST_CONFIG.init(config);

    let pty_path = instance.get_pty();

    log::info!("Connecting to {pty_path}");

    let serial_stream =
        tokio_serial::SerialStream::open(&tokio_serial::new(pty_path, BAUD)).unwrap();
    let (runner_transport, mullvad_daemon_transport, mut connection_handle, completion_handle) =
        test_rpc::transport::create_client_transports(serial_stream).await?;

    if !skip_wait {
        connection_handle.wait_for_server().await?;
    }

    log::info!("Running client");

    let client = ServiceClient::new(connection_handle.clone(), runner_transport);
    let mullvad_client =
        mullvad_daemon::new_rpc_client(connection_handle, mullvad_daemon_transport).await;

    let mut tests: Vec<_> = inventory::iter::<tests::TestMetadata>().collect();
    tests.sort_by_key(|test| test.priority.unwrap_or(0));

    if !test_filters.is_empty() {
        tests.retain(|test| {
            if test.always_run {
                return true;
            }
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
        let mut mclient = mullvad_client.as_type(test.mullvad_client_version).await;

        if let Some(client) = mclient.downcast_mut::<ManagementServiceClient>() {
            crate::tests::init_default_settings(client).await;
        }

        log::info!("Running {}", test.name);
        let test_result = run_test(client.clone(), mclient, &test.func, test.name)
            .await
            .context("Failed to run test")?;

        if test.mullvad_client_version == MullvadClientVersion::New {
            // Try to reset the daemon state if the test failed OR if the test doesn't explicitly
            // disabled cleanup.
            if test.cleanup || matches!(test_result.result, Err(_) | Ok(Err(_))) {
                let mut client = mullvad_client.new_client().await;
                crate::tests::cleanup_after_test(&mut client).await?;
            }
        }

        test_result.print();

        match test_result.result {
            Err(panic) => {
                final_result = Err(panic).context("test panicked");
                if test.must_succeed {
                    break;
                }
            }
            Ok(Err(failure)) => {
                final_result = Err(failure).context("test failed");
                if test.must_succeed {
                    break;
                }
            }
            Ok(Ok(result)) => {
                final_result = final_result.and(Ok(result));
            }
        }
    }

    // wait for cleanup
    drop(mullvad_client);
    let _ = tokio::time::timeout(Duration::from_secs(5), completion_handle).await;

    final_result
}
