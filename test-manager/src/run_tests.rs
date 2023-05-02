use crate::{logging::run_test, mullvad_daemon, tests, vm};
use anyhow::{Context, Result};
use mullvad_management_interface::ManagementServiceClient;
use std::time::Duration;
use test_rpc::{mullvad_daemon::MullvadClientVersion, ServiceClient};
use crate::tests::TestContext;

const BAUD: u32 = 115200;

pub async fn run(
    config: tests::config::TestConfig,
    instance: &dyn vm::VmInstance,
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

    let test_context = TestContext {
        rpc_provider: mullvad_client,
    };

    let mut successful_tests = vec![];
    let mut failed_tests = vec![];

    for test in tests {
        let mut mclient = test_context.rpc_provider.as_type(test.mullvad_client_version).await;

        if let Some(client) = mclient.downcast_mut::<ManagementServiceClient>() {
            crate::tests::init_default_settings(client).await;
        }

        log::info!("Running {}", test.name);
        let test_result = run_test(client.clone(), mclient, &test.func, test.name, test_context.clone())
            .await
            .context("Failed to run test")?;

        if test.mullvad_client_version == MullvadClientVersion::New {
            // Try to reset the daemon state if the test failed OR if the test doesn't explicitly
            // disabled cleanup.
            if test.cleanup || matches!(test_result.result, Err(_) | Ok(Err(_))) {
                let mut client = test_context.rpc_provider.new_client().await;
                crate::tests::cleanup_after_test(&mut client).await?;
            }
        }

        test_result.print();

        match test_result.result {
            Err(panic) => {
                failed_tests.push(test.name);
                final_result = Err(panic).context("test panicked");
                if test.must_succeed {
                    break;
                }
            }
            Ok(Err(failure)) => {
                failed_tests.push(test.name);
                final_result = Err(failure).context("test failed");
                if test.must_succeed {
                    break;
                }
            }
            Ok(Ok(result)) => {
                successful_tests.push(test.name);
                final_result = final_result.and(Ok(result));
            }
        }
    }

    println!("TESTS THAT SUCCEEDED:");
    for test in successful_tests {
        println!("{test}");
    }

    println!("TESTS THAT FAILED:");
    for test in failed_tests {
        println!("{test}");
    }

    // wait for cleanup
    drop(test_context);
    let _ = tokio::time::timeout(Duration::from_secs(5), completion_handle).await;

    final_result
}
