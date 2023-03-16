use super::helpers::{get_package_desc, ping_with_timeout, AbortOnDrop};
use super::Error;
use crate::get_possible_api_endpoints;

use crate::config::*;
use crate::network_monitor::{start_packet_monitor, MonitorOptions};
use mullvad_management_interface::types;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
use test_macro::test_function;
use test_rpc::{mullvad_daemon::ServiceStatus, Interface, ServiceClient};

/// Install the last stable version of the app and verify that it is running.
#[test_function(priority = -200)]
pub async fn test_install_previous_app(rpc: ServiceClient) -> Result<(), Error> {
    // verify that daemon is not already running
    if rpc.mullvad_daemon_get_status().await? != ServiceStatus::NotRunning {
        return Err(Error::DaemonRunning);
    }

    // install package
    log::debug!("Installing old app");
    rpc.install_app(get_package_desc(&rpc, &PREVIOUS_APP_FILENAME).await?)
        .await?;

    // verify that daemon is running
    if rpc.mullvad_daemon_get_status().await? != ServiceStatus::Running {
        return Err(Error::DaemonNotRunning);
    }

    Ok(())
}

/// Upgrade to the "version under test". This test fails if:
///
/// * Outgoing traffic whose destination is not one of the bridge
///   relays or the API is detected during the upgrade.
/// * Leaks (TCP/UDP/ICMP) to a single public IP address are
///   successfully produced during the upgrade.
/// * The installer does not successfully complete.
/// * The VPN service is not running after the upgrade.
#[test_function(priority = -190)]
pub async fn test_upgrade_app(
    rpc: ServiceClient,
    mut mullvad_client: old_mullvad_management_interface::ManagementServiceClient,
) -> Result<(), Error> {
    let inet_destination: SocketAddr = "1.1.1.1:1337".parse().unwrap();
    let bind_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();

    // Give it some time to start
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Verify that daemon is running
    if rpc.mullvad_daemon_get_status().await? != ServiceStatus::Running {
        return Err(Error::DaemonNotRunning);
    }

    // Login to test preservation of device/account
    // FIXME: This can fail due to throttling as well
    mullvad_client
        .login_account(ACCOUNT_TOKEN.clone())
        .await
        .expect("login failed");

    //
    // Start blocking
    //
    log::debug!("Entering blocking error state");

    mullvad_client
        .update_relay_settings(
            old_mullvad_management_interface::types::RelaySettingsUpdate {
                r#type: Some(
                    old_mullvad_management_interface::types::relay_settings_update::Type::Normal(
                        old_mullvad_management_interface::types::NormalRelaySettingsUpdate {
                            location: Some(
                                old_mullvad_management_interface::types::RelayLocation {
                                    country: "xx".to_string(),
                                    city: "".to_string(),
                                    hostname: "".to_string(),
                                },
                            ),
                            ..Default::default()
                        },
                    ),
                ),
            },
        )
        .await
        .map_err(|error| Error::DaemonError(format!("Failed to set relay settings: {}", error)))?;

    // cannot use the event listener due since the proto file is incompatible
    mullvad_client
        .connect_tunnel(())
        .await
        .expect("failed to begin connecting");
    tokio::time::timeout(super::WAIT_FOR_TUNNEL_STATE_TIMEOUT, async {
        loop {
            // use polling for sake of simplicity
            if matches!(
                mullvad_client
                    .get_tunnel_state(())
                    .await
                    .expect("RPC error")
                    .into_inner(),
                old_mullvad_management_interface::types::TunnelState {
                    state: Some(
                        old_mullvad_management_interface::types::tunnel_state::State::Error { .. }
                    ),
                }
            ) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
    .await
    .map_err(|_error| Error::DaemonError(String::from("Failed to enter blocking error state")))?;

    //
    // Begin monitoring outgoing traffic and pinging
    //

    let guest_ip = rpc
        .get_interface_ip(Interface::NonTunnel)
        .await
        .expect("failed to obtain tunnel IP");
    log::debug!("Guest IP: {guest_ip}");

    let api_endpoints = get_possible_api_endpoints!(&mut mullvad_client)?;

    log::debug!("Monitoring outgoing traffic");

    let monitor = start_packet_monitor(
        move |packet| {
            packet.source.ip() == guest_ip && !api_endpoints.contains(&packet.destination.ip())
        },
        MonitorOptions::default(),
    )
    .await;

    let ping_rpc = rpc.clone();
    let abort_on_drop = AbortOnDrop(tokio::spawn(async move {
        loop {
            let _ = ping_rpc.send_tcp(None, bind_addr, inet_destination).await;
            let _ = ping_rpc.send_udp(None, bind_addr, inet_destination).await;
            let _ = ping_with_timeout(&ping_rpc, inet_destination.ip(), None).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }));

    // install new package
    log::debug!("Installing new app");
    rpc.install_app(get_package_desc(&rpc, &CURRENT_APP_FILENAME).await?)
        .await?;

    // Give it some time to start
    tokio::time::sleep(Duration::from_secs(3)).await;

    // verify that daemon is running
    if rpc.mullvad_daemon_get_status().await? != ServiceStatus::Running {
        return Err(Error::DaemonNotRunning);
    }

    //
    // Check if any traffic was observed
    //
    drop(abort_on_drop);
    let monitor_result = monitor.into_result().await.unwrap();
    assert_eq!(
        monitor_result.packets.len(),
        0,
        "observed unexpected packets from {guest_ip}"
    );

    Ok(())
}

/// Do some post-upgrade checks:
///
/// * Sanity check settings. This makes sure that the
///   settings weren't totally wiped.
/// * Verify that the account history still contains
///   the account number of the active account.
///
/// # Limitations
///
/// It doesn't try to check the correctness of all migration
/// logic. We have unit tests for that.
#[test_function(priority = -180)]
pub async fn test_post_upgrade(
    _rpc: ServiceClient,
    mut mullvad_client: mullvad_management_interface::ManagementServiceClient,
) -> Result<(), Error> {
    // check if settings were (partially) preserved
    log::info!("Sanity checking settings");

    let settings = mullvad_client
        .get_settings(())
        .await
        .expect("failed to obtain settings")
        .into_inner();

    const EXPECTED_COUNTRY: &str = "xx";

    let relay_location_was_preserved = match &settings.relay_settings {
        Some(types::RelaySettings {
            endpoint:
                Some(types::relay_settings::Endpoint::Normal(types::NormalRelaySettings {
                    location:
                        Some(mullvad_management_interface::types::RelayLocation { country, .. }),
                    ..
                })),
        }) => country == EXPECTED_COUNTRY,
        _ => false,
    };

    assert!(
        relay_location_was_preserved,
        "relay location was not preserved after upgrade. new settings: {:?}",
        settings,
    );

    // check if account history was preserved
    let history = mullvad_client
        .get_account_history(())
        .await
        .expect("failed to obtain account history");
    assert_eq!(
        history.into_inner().token,
        Some(ACCOUNT_TOKEN.clone()),
        "lost account history"
    );

    // TODO: check version

    Ok(())
}

/// Uninstall the app version being tested. This verifies
/// that that the uninstaller works, and also that logs,
/// application files, system services are removed.
/// It also tests whether the device is removed from
/// the account.
///
/// # Limitations
///
/// Files due to Electron, temporary files, registry
/// values/keys, and device drivers are not guaranteed
/// to be deleted.
#[test_function(priority = -170, cleanup = false)]
pub async fn test_uninstall_app(
    rpc: ServiceClient,
    mut mullvad_client: mullvad_management_interface::ManagementServiceClient,
) -> Result<(), Error> {
    const THROTTLE_RETRY_DELAY: Duration = Duration::from_secs(120);

    if rpc.mullvad_daemon_get_status().await? != ServiceStatus::Running {
        return Err(Error::DaemonNotRunning);
    }

    // save device to verify that uninstalling removes the device
    // we should still be logged in after upgrading
    let uninstalled_device = mullvad_client
        .get_device(())
        .await
        .expect("failed to get device data")
        .into_inner();
    let uninstalled_device = uninstalled_device
        .device
        .expect("missing account/device")
        .device
        .expect("missing device id")
        .id;

    log::debug!("Uninstalling app");
    rpc.uninstall_app().await?;

    let app_traces = rpc
        .find_mullvad_app_traces()
        .await
        .expect("failed to obtain remaining Mullvad files");
    assert!(
        app_traces.is_empty(),
        "found files after uninstall: {app_traces:?}"
    );

    if rpc.mullvad_daemon_get_status().await? != ServiceStatus::NotRunning {
        return Err(Error::DaemonRunning);
    }

    // verify that device was removed
    let api = mullvad_api::Runtime::new(tokio::runtime::Handle::current())
        .expect("failed to create api runtime");
    let rest_handle = api
        .mullvad_rest_handle(
            mullvad_api::proxy::ApiConnectionMode::Direct.into_repeat(),
            |_| async { true },
        )
        .await;
    let device_client = mullvad_api::DevicesProxy::new(rest_handle);

    let devices = loop {
        match device_client.list(ACCOUNT_TOKEN.clone()).await {
            Ok(devices) => break Ok(devices),
            // Work around throttling errors by sleeping
            Err(mullvad_api::rest::Error::ApiError(
                mullvad_api::rest::StatusCode::TOO_MANY_REQUESTS,
                _,
            )) => {
                log::debug!(
                    "Device list fetch failed due to throttling. Sleeping for {} seconds",
                    THROTTLE_RETRY_DELAY.as_secs()
                );

                tokio::time::sleep(THROTTLE_RETRY_DELAY).await;
            }
            Err(error) => break Err(error),
        }
    }
    .expect("failed to obtain device list");

    // FIXME: This assertion can fail due to throttling as well
    assert!(
        !devices.iter().any(|device| device.id == uninstalled_device),
        "device id {} still exists after uninstall",
        uninstalled_device,
    );

    Ok(())
}

/// Install the app cleanly, failing if the installer doesn't succeed
/// or if the VPN service is not running afterwards.
#[test_function(priority = -160)]
pub async fn test_install_new_app(rpc: ServiceClient) -> Result<(), Error> {
    // verify that daemon is not already running
    if rpc.mullvad_daemon_get_status().await? != ServiceStatus::NotRunning {
        return Err(Error::DaemonRunning);
    }

    // install package
    log::debug!("Installing new app");
    rpc.install_app(get_package_desc(&rpc, &CURRENT_APP_FILENAME).await?)
        .await?;

    // verify that daemon is running
    if rpc.mullvad_daemon_get_status().await? != ServiceStatus::Running {
        return Err(Error::DaemonNotRunning);
    }

    Ok(())
}
