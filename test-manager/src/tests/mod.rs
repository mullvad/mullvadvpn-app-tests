mod test_metadata;

use crate::network_monitor::{start_packet_monitor, MonitorOptions};
use mullvad_management_interface::{
    types::{self, RelayLocation},
    ManagementServiceClient,
};
use mullvad_types::{
    relay_constraints::{
        Constraint, LocationConstraint, RelayConstraintsUpdate, RelaySettingsUpdate,
        WireguardConstraints,
    },
    states::TunnelState,
};
use pnet_packet::ip::IpNextHeaderProtocols;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    time::{Duration, SystemTime},
};
use talpid_types::net::{Endpoint, IpVersion, TransportProtocol, TunnelEndpoint, TunnelType};
use tarpc::context;
use test_macro::test_module;
use test_rpc::{
    meta,
    mullvad_daemon::ServiceStatus,
    package::{Package, PackageType},
    Interface, ServiceClient,
};
use tokio::time::timeout;

const PING_TIMEOUT: Duration = Duration::from_secs(3);
const WAIT_FOR_TUNNEL_STATE_TIMEOUT: Duration = Duration::from_secs(20);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(err_derive::Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error(display = "RPC call failed")]
    Rpc(#[source] tarpc::client::RpcError),

    #[error(display = "Timeout waiting for ping")]
    PingTimeout,

    #[error(display = "Failed to ping destination")]
    PingFailed,

    #[error(display = "Package action failed")]
    Package(&'static str, test_rpc::package::Error),

    #[error(display = "Found running daemon unexpectedly")]
    DaemonRunning,

    #[error(display = "Daemon unexpectedly not running")]
    DaemonNotRunning,

    #[error(display = "The daemon returned an error: {}", _0)]
    DaemonError(String),
}

macro_rules! assert_tunnel_state {
    ($mullvad_client:expr, $pattern:pat) => {{
        let state = get_tunnel_state($mullvad_client).await;
        assert!(matches!(state, $pattern), "state: {:?}", state);
    }};
}

/// Return all possible API endpoints. Note that this includes all bridge IPs. Ideally,
/// we'd keep track of the current API IP, not exonerate all bridges from being considered
/// leaky.
macro_rules! get_possible_api_endpoints {
    ($mullvad_client:expr) => {{
        // TODO: Remove old API endpoint
        let mut api_endpoints = vec![
            IpAddr::V4(Ipv4Addr::new(45, 83, 222, 100)),
            IpAddr::V4(Ipv4Addr::new(45, 83, 223, 196)),
        ];

        let relay_list = $mullvad_client
            .get_relay_locations(())
            .await
            .map_err(|error| Error::DaemonError(format!("Failed to obtain relay list: {}", error)))?
            .into_inner();

        api_endpoints.extend(
            relay_list
                .countries
                .into_iter()
                .flat_map(|country| country.cities)
                .filter_map(|mut city| {
                    city.relays.retain(|relay| {
                        relay.active
                            && relay.endpoint_type == (types::relay::RelayType::Bridge as i32)
                    });
                    if !city.relays.is_empty() {
                        Some(city)
                    } else {
                        None
                    }
                })
                .flat_map(|city| {
                    city.relays
                        .into_iter()
                        .map(|relay| IpAddr::V4(relay.ipv4_addr_in.parse().expect("invalid IP")))
                }),
        );

        Ok::<Vec<IpAddr>, Error>(api_endpoints)
    }};
}

#[test_module]
pub mod manager_tests {
    use mullvad_types::relay_constraints::{OpenVpnConstraints, TransportPort};

    use super::*;

    #[manager_test(priority = -6)]
    pub async fn test_install_previous_app(rpc: ServiceClient) -> Result<(), Error> {
        // verify that daemon is not already running
        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::NotRunning {
            return Err(Error::DaemonRunning);
        }

        // install package
        let mut ctx = context::current();
        ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

        rpc.install_app(ctx, get_package_desc(&rpc, "previous-app").await?)
            .await?
            .map_err(|err| Error::Package("previous app", err))?;

        // verify that daemon is running
        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::Running {
            return Err(Error::DaemonNotRunning);
        }

        Ok(())
    }

    #[manager_test(priority = -5)]
    pub async fn test_upgrade_app(
        rpc: ServiceClient,
        mut mullvad_client: old_mullvad_management_interface::ManagementServiceClient,
    ) -> Result<(), Error> {
        const PING_DESTINATION: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

        // Verify that daemon is running
        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::Running {
            return Err(Error::DaemonNotRunning);
        }

        // Give it some time to start
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Login to test preservation of device/account
        mullvad_client.login_account(account_token()).await.expect("login failed");

        //
        // Start blocking
        //
        log::debug!("Entering blocking error state");

        mullvad_client
            .update_relay_settings(old_mullvad_management_interface::types::RelaySettingsUpdate {
                r#type: Some(old_mullvad_management_interface::types::relay_settings_update::Type::Normal(
                    old_mullvad_management_interface::types::NormalRelaySettingsUpdate {
                        location: Some(old_mullvad_management_interface::types::RelayLocation {
                            country: "xx".to_string(),
                            city: "".to_string(),
                            hostname: "".to_string(),
                        }),
                        ..Default::default()
                    }
                )),
            })
            .await
            .map_err(|error| {
                Error::DaemonError(format!("Failed to set relay settings: {}", error))
            })?;

        // cannot use the event listener due since the proto file is incompatible
        mullvad_client
            .connect_tunnel(())
            .await
            .expect("failed to begin connecting");
        tokio::time::sleep(Duration::from_secs(1)).await;

        //
        // Begin monitoring outgoing traffic and pinging
        //

        let guest_ip = rpc
            .get_interface_ip(context::current(), Interface::NonTunnel)
            .await?
            .expect("failed to obtain tunnel IP");
        log::debug!("Guest IP: {guest_ip}");

        let api_endpoints = get_possible_api_endpoints!(&mut mullvad_client)?;

        log::debug!("Monitoring outgoing traffic");

        let monitor = start_packet_monitor(
            move |packet| {
                packet.source.ip() == guest_ip && !api_endpoints.contains(&packet.destination.ip())
            },
            MonitorOptions::default(),
        );

        let ping_rpc = rpc.clone();
        let abort_on_drop = AbortOnDrop(tokio::spawn(async move {
            let _ = ping_rpc
                .send_ping(context::current(), None, PING_DESTINATION)
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        }));

        // install new package
        let mut ctx = context::current();
        ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

        rpc.install_app(ctx, get_package_desc(&rpc, "current-app").await?)
            .await?
            .map_err(|error| Error::Package("current app", error))?;

        // verify that daemon is running
        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::Running {
            return Err(Error::DaemonNotRunning);
        }

        //
        // Check if any traffic was observed
        //
        drop(abort_on_drop);
        let monitor_result = monitor.into_result().await.unwrap();
        assert_eq!(
            monitor_result.matching_packets, 0,
            "observed unexpected packets from {guest_ip}"
        );

        Ok(())
    }

    #[manager_test(priority = -4)]
    pub async fn test_post_upgrade(
        _rpc: ServiceClient,
        mut mullvad_client: mullvad_management_interface::ManagementServiceClient,
    ) -> Result<(), Error> {
        // check if settings were (partially) preserved
        log::info!("Sanity checking settings");

        let settings = mullvad_client.get_settings(()).await.expect("failed to obtain settings").into_inner();

        const EXPECTED_COUNTRY: &str = "xx";

        let relay_location_was_preserved = match &settings.relay_settings {
            Some(types::RelaySettings {
                endpoint: Some(types::relay_settings::Endpoint::Normal(
                    types::NormalRelaySettings {
                        location: Some(mullvad_management_interface::types::RelayLocation {
                            country,
                            ..
                        }),
                        ..
                    }
                )),
            }) => {
                country == EXPECTED_COUNTRY
            }
            _ => false,
        };

        assert!(
            relay_location_was_preserved,
            "relay location was not preserved after upgrade. new settings: {:?}",
            settings,
        );

        // check if account history was preserved
        let history = mullvad_client.get_account_history(()).await.expect("failed to obtain account history");
        let expected_account = account_token();
        assert_eq!(history.into_inner().token, Some(expected_account), "lost account history");

        // TODO: check version

        Ok(())
    }

    #[manager_test(priority = -3)]
    pub async fn test_uninstall_app(
        rpc: ServiceClient,
        mut mullvad_client: mullvad_management_interface::ManagementServiceClient,
    ) -> Result<(), Error> {
        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::Running {
            return Err(Error::DaemonNotRunning);
        }

        let mut ctx = context::current();
        ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

        // save device to verify that uninstalling removes the device
        // we should still be logged in after upgrading
        let uninstalled_device = mullvad_client.get_device(()).await.expect("failed to get device data").into_inner();
        let uninstalled_device = uninstalled_device.device.expect("missing account/device").device.expect("missing device id").id;

        rpc.uninstall_app(ctx)
            .await?
            .map_err(|error| Error::Package("uninstall app", error))?;

        let app_traces = rpc.find_mullvad_app_traces(context::current()).await?
            .expect("failed to obtain remaining Mullvad files");
        assert!(app_traces.is_empty(), "found files after uninstall: {app_traces:?}");

        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::NotRunning {
            return Err(Error::DaemonRunning);
        }

        // verify that device was removed
        let api = mullvad_api::Runtime::new(tokio::runtime::Handle::current()).expect("failed to create api runtime");
        let rest_handle = api.mullvad_rest_handle(
            mullvad_api::proxy::ApiConnectionMode::Direct.into_repeat(),
            |_| async { true },
        ).await;
        let device_client = mullvad_api::DevicesProxy::new(rest_handle);

        let devices = device_client.list(account_token()).await.expect("failed to list devices");

        assert!(
            devices.iter().find(|device| device.id == uninstalled_device).is_none(),
            "device id {} still exists after uninstall",
            uninstalled_device,
        );

        Ok(())
    }

    #[manager_test(priority = -2)]
    pub async fn test_install_new_app(rpc: ServiceClient) -> Result<(), Error> {
        // verify that daemon is not already running
        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::NotRunning {
            return Err(Error::DaemonRunning);
        }

        // install package
        let mut ctx = context::current();
        ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

        rpc.install_app(ctx, get_package_desc(&rpc, "current-app").await?)
            .await?
            .map_err(|err| Error::Package("current app", err))?;

        // verify that daemon is running
        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::Running {
            return Err(Error::DaemonNotRunning);
        }

        Ok(())
    }

    async fn get_package_desc(rpc: &ServiceClient, name: &str) -> Result<Package, Error> {
        match rpc.get_os(context::current()).await.map_err(Error::Rpc)? {
            meta::Os::Linux => Ok(Package {
                r#type: PackageType::Dpkg,
                path: Path::new(&format!("/opt/testing/{}.deb", name)).to_path_buf(),
            }),
            meta::Os::Windows => Ok(Package {
                r#type: PackageType::NsisExe,
                path: Path::new(&format!(r"E:\{}.exe", name)).to_path_buf(),
            }),
            _ => unimplemented!(),
        }
    }

    #[manager_test(priority = -1)]
    pub async fn test_ping_while_disconnected(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        const PING_DESTINATION: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        //
        // Ping while disconnected
        //

        log::info!("Ping {}", PING_DESTINATION);

        let monitor = start_packet_monitor(
            |packet| packet.destination.ip() == PING_DESTINATION,
            MonitorOptions {
                stop_on_match: true,
                stop_on_non_match: false,
                timeout: Some(PING_TIMEOUT),
            },
        );

        rpc.send_ping(
            context::current(),
            Some(Interface::NonTunnel),
            PING_DESTINATION,
        )
        .await
        .map_err(Error::Rpc)?
        .expect("Disconnected ping failed");

        assert_eq!(
            monitor
                .wait()
                .await
                .expect("monitor stopped")
                .matching_packets,
            1,
        );

        Ok(())
    }

    #[manager_test(priority = -1)]
    pub async fn test_login(
        _rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        // TODO: Test too many devices, removal, etc.

        log::info!("Logging in/generating device");

        let account = account_token();

        mullvad_client
            .login_account(account)
            .await
            .expect("login failed");

        // TODO: verify that device exists

        Ok(())
    }

    #[manager_test(priority = 100)]
    pub async fn test_logout(
        _rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        log::info!("Removing device");

        mullvad_client
            .logout_account(())
            .await
            .expect("logout failed");

        // TODO: verify that the device was deleted

        Ok(())
    }

    pub fn account_token() -> String {
        std::env::var("ACCOUNT_TOKEN").expect("ACCOUNT_TOKEN is unspecified")
    }

    #[manager_test]
    pub async fn test_connect_relay(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        // TODO: Since we're connected to an actual relay, a real IP must be used here.
        const PING_DESTINATION: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        //
        // Set relay to use
        //

        log::info!("Select relay");

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            location: Some(Constraint::Only(LocationConstraint::Hostname(
                "se".to_string(),
                "got".to_string(),
                "se9-wireguard".to_string(),
            ))),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to update relay settings");

        //
        // Connect
        //

        // TODO: Obtain IP from relay list
        const EXPECTED_RELAY_IP: Ipv4Addr = Ipv4Addr::new(185, 213, 154, 68);

        connect_and_wait(&mut mullvad_client).await?;

        let state = get_tunnel_state(&mut mullvad_client).await;

        //
        // Verify that endpoint was selected
        //

        match state {
            TunnelState::Connected {
                endpoint:
                    TunnelEndpoint {
                        endpoint:
                            Endpoint {
                                address: SocketAddr::V4(addr),
                                protocol: TransportProtocol::Udp,
                            },
                        tunnel_type: TunnelType::Wireguard,
                        quantum_resistant: false,
                        proxy: None,
                        obfuscation: None,
                        entry_endpoint: None,
                    },
                ..
            } => {
                assert_eq!(addr.ip(), &EXPECTED_RELAY_IP);
            }
            actual => panic!("unexpected tunnel state: {:?}", actual),
        }

        //
        // Ping outside of tunnel while connected
        //

        log::info!("Ping outside tunnel (fail)");

        let ping_result =
            ping_with_timeout(&rpc, PING_DESTINATION, Some(Interface::NonTunnel)).await;
        assert!(ping_result.is_err(), "ping result: {:?}", ping_result);

        //
        // Ping inside tunnel while connected
        //

        log::info!("Ping inside tunnel");

        assert_eq!(
            ping_with_timeout(&rpc, PING_DESTINATION, Some(Interface::Tunnel),).await,
            Ok(()),
        );

        disconnect_and_wait(&mut mullvad_client).await?;

        Ok(())
    }

    #[manager_test]
    pub async fn test_connect_openvpn_relay(
        _rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        // TODO: Add packet monitor

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        const CONSTRAINTS: [(&str, Constraint<TransportPort>); 3] = [
            ("any", Constraint::Any),
            (
                "UDP",
                Constraint::Only(TransportPort {
                    protocol: TransportProtocol::Udp,
                    port: Constraint::Any,
                }),
            ),
            (
                "TCP",
                Constraint::Only(TransportPort {
                    protocol: TransportProtocol::Tcp,
                    port: Constraint::Any,
                }),
            ),
        ];

        for (protocol, constraint) in CONSTRAINTS {
            log::info!("Connect to {protocol} OpenVPN endpoint");

            let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
                location: Some(Constraint::Only(LocationConstraint::Country(
                    "se".to_string(),
                ))),
                tunnel_protocol: Some(Constraint::Only(TunnelType::OpenVpn)),
                openvpn_constraints: Some(OpenVpnConstraints { port: constraint }),
                ..Default::default()
            });

            update_relay_settings(&mut mullvad_client, relay_settings)
                .await
                .expect("failed to update relay settings");

            connect_and_wait(&mut mullvad_client).await?;

            disconnect_and_wait(&mut mullvad_client).await?;
        }

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            tunnel_protocol: Some(Constraint::Any),
            openvpn_constraints: Some(OpenVpnConstraints::default()),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to reset relay settings");

        Ok(())
    }

    #[manager_test]
    pub async fn test_connect_wireguard_relay(
        _rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        // TODO: Add packet monitor
        // TODO: IPv6

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        const PORTS: [(u16, bool); 3] = [(53, true), (51820, true), (1, false)];

        for (port, should_succeed) in PORTS {
            log::info!("Connect to WireGuard endpoint on port {port}");

            let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
                location: Some(Constraint::Only(LocationConstraint::Country(
                    "se".to_string(),
                ))),
                tunnel_protocol: Some(Constraint::Only(TunnelType::Wireguard)),
                wireguard_constraints: Some(WireguardConstraints {
                    port: Constraint::Only(port),
                    ..Default::default()
                }),
                ..Default::default()
            });

            update_relay_settings(&mut mullvad_client, relay_settings)
                .await
                .expect("failed to update relay settings");

            assert_eq!(
                connect_and_wait(&mut mullvad_client).await.is_ok(),
                should_succeed,
                "unexpected result for port {port}",
            );

            disconnect_and_wait(&mut mullvad_client).await?;
        }

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            tunnel_protocol: Some(Constraint::Any),
            wireguard_constraints: Some(WireguardConstraints::default()),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to reset relay settings");

        Ok(())
    }

    #[manager_test]
    pub async fn test_connect_udp2tcp_relay(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        // TODO: Since we're connected to an actual relay, a real IP must be used here.
        const PING_DESTINATION: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        mullvad_client
            .set_obfuscation_settings(types::ObfuscationSettings {
                selected_obfuscation: i32::from(
                    types::obfuscation_settings::SelectedObfuscation::Udp2tcp,
                ),
                udp2tcp: Some(types::Udp2TcpObfuscationSettings { port: 0 }),
            })
            .await
            .expect("failed to enable udp2tcp");

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            location: Some(Constraint::Only(LocationConstraint::Country(
                "se".to_string(),
            ))),
            tunnel_protocol: Some(Constraint::Only(TunnelType::Wireguard)),
            wireguard_constraints: Some(WireguardConstraints::default()),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to update relay settings");

        log::info!("Connect to WireGuard via tcp2udp endpoint");

        connect_and_wait(&mut mullvad_client).await?;

        //
        // Set up packet monitor
        // TODO: make sure at least one packet from the non-tun iface is received
        //

        let guest_ip = rpc
            .get_interface_ip(context::current(), Interface::NonTunnel)
            .await?
            .expect("failed to obtain inet interface IP");

        let monitor = start_packet_monitor(
            move |packet| {
                packet.source.ip() != guest_ip || (packet.protocol == IpNextHeaderProtocols::Tcp)
            },
            MonitorOptions::default(),
        );

        //
        // Verify that we can ping stuff
        //

        log::info!("Ping {}", PING_DESTINATION);

        ping_with_timeout(&rpc, PING_DESTINATION, Some(Interface::Tunnel))
            .await
            .expect("Failed to ping internet target");

        let monitor_result = monitor.into_result().await.unwrap();
        assert!(
            monitor_result.non_matching_packets == 0,
            "non_matching_packets: {}",
            monitor_result.non_matching_packets
        );

        //
        // Disconnect
        //

        disconnect_and_wait(&mut mullvad_client).await?;

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            tunnel_protocol: Some(Constraint::Any),
            wireguard_constraints: Some(WireguardConstraints::default()),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to reset relay settings");

        mullvad_client
            .set_obfuscation_settings(types::ObfuscationSettings {
                selected_obfuscation: i32::from(
                    types::obfuscation_settings::SelectedObfuscation::Off,
                ),
                udp2tcp: Some(types::Udp2TcpObfuscationSettings { port: 0 }),
            })
            .await
            .expect("failed to disable udp2tcp");

        Ok(())
    }

    #[manager_test]
    pub async fn test_bridge(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        const EXPECTED_EXIT_HOSTNAME: &str = "se-got-006";
        const EXPECTED_ENTRY_IP: Ipv4Addr = Ipv4Addr::new(185, 213, 154, 117);

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        //
        // Enable bridge mode
        //

        log::info!("Updating bridge settings");

        mullvad_client
            .set_bridge_state(types::BridgeState {
                state: i32::from(types::bridge_state::State::On),
            })
            .await
            .expect("failed to enable bridge mode");

        mullvad_client
            .set_bridge_settings(types::BridgeSettings {
                r#type: Some(types::bridge_settings::Type::Normal(
                    types::bridge_settings::BridgeConstraints {
                        location: Some(types::RelayLocation {
                            country: "se".to_string(),
                            city: "got".to_string(),
                            hostname: "se-got-br-001".to_string(),
                        }),
                        providers: vec![],
                        ownership: i32::from(types::Ownership::Any),
                    },
                )),
            })
            .await
            .expect("failed to update bridge settings");

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            location: Some(Constraint::Only(LocationConstraint::Hostname(
                "se".to_string(),
                "got".to_string(),
                EXPECTED_EXIT_HOSTNAME.to_string(),
            ))),
            tunnel_protocol: Some(Constraint::Only(TunnelType::OpenVpn)),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to update relay settings");

        //
        // Connect to VPN
        //

        log::info!("Connect to OpenVPN relay via bridge");

        let monitor = start_packet_monitor(
            |packet| packet.destination.ip() == EXPECTED_ENTRY_IP,
            MonitorOptions::default(),
        );

        connect_and_wait(&mut mullvad_client).await?;

        //
        // Verify entry IP
        //

        log::info!("Verifying entry server");

        let monitor_result = monitor.into_result().await.unwrap();
        assert!(
            monitor_result.matching_packets > 0,
            "matching_packets: {}",
            monitor_result.matching_packets
        );

        //
        // Verify exit IP
        //

        log::info!("Verifying exit server");

        let geoip = rpc
            .geoip_lookup(context::current())
            .await
            .expect("geoip lookup failed")
            .expect("geoip lookup failed");

        assert_eq!(geoip.mullvad_exit_ip_hostname, EXPECTED_EXIT_HOSTNAME);

        //
        // Disconnect
        //

        disconnect_and_wait(&mut mullvad_client).await?;

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            location: Some(Constraint::Any),
            tunnel_protocol: Some(Constraint::Any),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to reset relay settings");

        mullvad_client
            .set_bridge_state(types::BridgeState {
                state: i32::from(types::bridge_state::State::Auto),
            })
            .await
            .expect("failed to reset bridge mode");

        Ok(())
    }

    #[manager_test]
    pub async fn test_lan(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        const PING_DESTINATION: IpAddr = IpAddr::V4(Ipv4Addr::new(172, 29, 1, 200));

        //
        // Connect
        //

        connect_and_wait(&mut mullvad_client).await?;

        //
        // Disable LAN sharing
        //

        log::info!("LAN sharing: disabled");

        mullvad_client
            .set_allow_lan(false)
            .await
            .expect("failed to disable LAN sharing");

        //
        // Ensure LAN is not reachable
        //

        log::info!("Ping {} (LAN)", PING_DESTINATION);

        ping_with_timeout(&rpc, PING_DESTINATION, Some(Interface::NonTunnel))
            .await
            .expect_err("Successfully pinged LAN target");

        //
        // Enable LAN sharing
        //

        log::info!("LAN sharing: enabled");

        mullvad_client
            .set_allow_lan(true)
            .await
            .expect("failed to enable LAN sharing");

        //
        // Ensure LAN is reachable
        //

        log::info!("Ping {} (LAN)", PING_DESTINATION);

        ping_with_timeout(&rpc, PING_DESTINATION, Some(Interface::NonTunnel))
            .await
            .expect("Failed to ping LAN target");

        disconnect_and_wait(&mut mullvad_client).await?;

        Ok(())
    }

    #[manager_test]
    pub async fn test_multihop(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        const EXPECTED_EXIT_HOSTNAME: &str = "se9-wireguard";
        const EXPECTED_ENTRY_IP: Ipv4Addr = Ipv4Addr::new(185, 213, 154, 66);

        //
        // Set relays to use
        //

        log::info!("Select relay");

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            location: Some(Constraint::Only(LocationConstraint::Hostname(
                "se".to_string(),
                "got".to_string(),
                EXPECTED_EXIT_HOSTNAME.to_string(),
            ))),
            wireguard_constraints: Some(WireguardConstraints {
                use_multihop: true,
                entry_location: Constraint::Only(LocationConstraint::Hostname(
                    "se".to_string(),
                    "got".to_string(),
                    "se3-wireguard".to_string(),
                )),
                ..Default::default()
            }),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to update relay settings");

        //
        // Connect
        //

        let monitor = start_packet_monitor(
            |packet| {
                packet.destination.ip() == EXPECTED_ENTRY_IP
                    && packet.protocol == IpNextHeaderProtocols::Udp
            },
            MonitorOptions::default(),
        );

        connect_and_wait(&mut mullvad_client).await?;

        //
        // Verify entry IP
        //

        log::info!("Verifying entry server");

        let monitor_result = monitor.into_result().await.unwrap();
        assert!(
            monitor_result.matching_packets > 0,
            "matching_packets: {}",
            monitor_result.matching_packets
        );

        //
        // Verify exit IP
        //

        log::info!("Verifying exit server");

        let geoip = rpc
            .geoip_lookup(context::current())
            .await
            .expect("geoip lookup failed")
            .expect("geoip lookup failed");

        assert_eq!(geoip.mullvad_exit_ip_hostname, EXPECTED_EXIT_HOSTNAME);

        disconnect_and_wait(&mut mullvad_client).await?;

        Ok(())
    }

    #[manager_test]
    pub async fn test_lockdown(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        const PING_LAN_DESTINATION: IpAddr = IpAddr::V4(Ipv4Addr::new(172, 29, 1, 200));
        const PING_INET_DESTINATION: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 3, 3, 7));

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        //
        // Enable lockdown mode
        //
        mullvad_client
            .set_block_when_disconnected(true)
            .await
            .expect("failed to enable lockdown mode");

        //
        // Disable LAN sharing
        //

        log::info!("LAN sharing: disabled");

        mullvad_client
            .set_allow_lan(false)
            .await
            .expect("failed to disable LAN sharing");

        //
        // Ensure all destinations are unreachable
        //

        ping_with_timeout(&rpc, PING_INET_DESTINATION, Some(Interface::NonTunnel))
            .await
            .expect_err("Successfully pinged internet target");

        ping_with_timeout(&rpc, PING_LAN_DESTINATION, Some(Interface::NonTunnel))
            .await
            .expect_err("Successfully pinged LAN target");

        //
        // Enable LAN sharing
        //

        log::info!("LAN sharing: enabled");

        mullvad_client
            .set_allow_lan(true)
            .await
            .expect("failed to enable LAN sharing");

        //
        // Ensure private IPs are reachable, but not others
        //

        ping_with_timeout(&rpc, PING_INET_DESTINATION, Some(Interface::NonTunnel))
            .await
            .expect_err("Successfully pinged internet target");

        ping_with_timeout(&rpc, PING_LAN_DESTINATION, Some(Interface::NonTunnel))
            .await
            .expect("Failed to ping LAN target");

        //
        // Connect
        //

        connect_and_wait(&mut mullvad_client).await?;

        //
        // Leak test
        //

        ping_with_timeout(&rpc, PING_INET_DESTINATION, Some(Interface::Tunnel))
            .await
            .expect_err("Successfully pinged internet target");

        ping_with_timeout(&rpc, PING_LAN_DESTINATION, Some(Interface::NonTunnel))
            .await
            .expect("Failed to ping LAN target");

        //
        // Disable lockdown mode
        //
        mullvad_client
            .set_block_when_disconnected(false)
            .await
            .expect("failed to disable lockdown mode");

        disconnect_and_wait(&mut mullvad_client).await?;

        Ok(())
    }
}

async fn ping_with_timeout(
    rpc: &ServiceClient,
    dest: IpAddr,
    interface: Option<Interface>,
) -> Result<(), Error> {
    timeout(
        PING_TIMEOUT,
        rpc.send_ping(context::current(), interface, dest),
    )
    .await
    .map_err(|_| Error::PingTimeout)?
    .map_err(Error::Rpc)?
    .map_err(|_| Error::PingFailed)
}

async fn connect_and_wait(mullvad_client: &mut ManagementServiceClient) -> Result<(), Error> {
    log::info!("Connecting");

    mullvad_client
        .connect_tunnel(())
        .await
        .map_err(|error| Error::DaemonError(format!("failed to begin connecting: {}", error)))?;

    let new_state = wait_for_tunnel_state(mullvad_client.clone(), |state| {
        matches!(
            state,
            TunnelState::Connected { .. } | TunnelState::Error(..)
        )
    })
    .await?;

    if matches!(new_state, TunnelState::Error(..)) {
        return Err(Error::DaemonError("daemon entered error state".to_string()));
    }

    log::info!("Connected");

    Ok(())
}

async fn disconnect_and_wait(mullvad_client: &mut ManagementServiceClient) -> Result<(), Error> {
    log::info!("Disconnecting");

    mullvad_client
        .disconnect_tunnel(())
        .await
        .map_err(|error| Error::DaemonError(format!("failed to begin disconnecting: {}", error)))?;
    wait_for_tunnel_state(mullvad_client.clone(), |state| {
        matches!(state, TunnelState::Disconnected)
    })
    .await?;

    log::info!("Disconnected");

    Ok(())
}

async fn wait_for_tunnel_state(
    rpc: mullvad_management_interface::ManagementServiceClient,
    accept_state_fn: impl Fn(&mullvad_types::states::TunnelState) -> bool,
) -> Result<mullvad_types::states::TunnelState, Error> {
    tokio::time::timeout(
        WAIT_FOR_TUNNEL_STATE_TIMEOUT,
        wait_for_tunnel_state_inner(rpc, accept_state_fn),
    )
    .await
    .map_err(|_error| Error::DaemonError(format!("Tunnel event listener timed out")))?
}

async fn wait_for_tunnel_state_inner(
    mut rpc: mullvad_management_interface::ManagementServiceClient,
    accept_state_fn: impl Fn(&mullvad_types::states::TunnelState) -> bool,
) -> Result<mullvad_types::states::TunnelState, Error> {
    let state = mullvad_types::states::TunnelState::try_from(
        rpc.get_tunnel_state(())
            .await
            .map_err(|error| {
                Error::DaemonError(format!("Failed to get tunnel state: {:?}", error))
            })?
            .into_inner(),
    )
    .map_err(|error| Error::DaemonError(format!("Invalid tunnel state: {:?}", error)))?;
    if accept_state_fn(&state) {
        return Ok(state);
    }

    match rpc.events_listen(()).await {
        Ok(events) => {
            let mut events = events.into_inner();
            loop {
                match events.message().await {
                    Ok(Some(event)) => match event.event.unwrap() {
                        mullvad_management_interface::types::daemon_event::Event::TunnelState(
                            new_state,
                        ) => {
                            let state = mullvad_types::states::TunnelState::try_from(new_state)
                                .map_err(|error| {
                                    Error::DaemonError(format!("Invalid tunnel state: {:?}", error))
                                })?;
                            if accept_state_fn(&state) {
                                return Ok(state);
                            }
                        }
                        _ => continue,
                    },
                    Ok(None) => break Err(Error::DaemonError(format!("Lost daemon event stream"))),
                    Err(status) => {
                        break Err(Error::DaemonError(format!(
                            "Failed to get next event: {}",
                            status
                        )))
                    }
                }
            }
        }
        Err(status) => Err(Error::DaemonError(format!(
            "Failed to get event stream: {}",
            status
        ))),
    }
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn update_relay_settings(
    mullvad_client: &mut ManagementServiceClient,
    relay_settings_update: RelaySettingsUpdate,
) -> Result<(), Error> {
    // TODO: implement from for RelaySettingsUpdate in mullvad_management_interface

    let update = match relay_settings_update {
        RelaySettingsUpdate::Normal(constraints) => {
            types::RelaySettingsUpdate {
                r#type: Some(types::relay_settings_update::Type::Normal(
                    types::NormalRelaySettingsUpdate {
                        location: constraints.location.map(types::RelayLocation::from),
                        providers: constraints
                            .providers
                            .map(|constraint| types::ProviderUpdate {
                                providers: constraint
                                    .map(|providers| providers.into_vec())
                                    .unwrap_or(vec![]),
                            }),
                        wireguard_constraints: constraints.wireguard_constraints.map(
                            |wireguard_constraints| types::WireguardConstraints {
                                ip_version: wireguard_constraints.ip_version.option().map(
                                    |ip_version| types::IpVersionConstraint {
                                        protocol: match ip_version {
                                            IpVersion::V4 => types::IpVersion::V4 as i32,
                                            IpVersion::V6 => types::IpVersion::V6 as i32,
                                        },
                                    },
                                ),
                                entry_location: Some(RelayLocation::from(
                                    wireguard_constraints.entry_location,
                                )),
                                port: u32::from(wireguard_constraints.port.unwrap_or(0)),
                                use_multihop: wireguard_constraints.use_multihop,
                            },
                        ),
                        // FIXME: Support more types here
                        ownership: None,
                        tunnel_type: None,
                        openvpn_constraints: None,
                    },
                )),
            }
        }
        RelaySettingsUpdate::CustomTunnelEndpoint(endpoint) => types::RelaySettingsUpdate {
            r#type: Some(types::relay_settings_update::Type::Custom(
                types::CustomRelaySettings {
                    host: endpoint.host.to_string(),
                    config: Some(types::ConnectionConfig::from(endpoint.config)),
                },
            )),
        },
    };

    mullvad_client
        .update_relay_settings(update)
        .await
        .map_err(|error| Error::DaemonError(format!("Failed to set relay settings: {}", error)))?;
    Ok(())
}

async fn get_tunnel_state(mullvad_client: &mut ManagementServiceClient) -> TunnelState {
    let state = mullvad_client
        .get_tunnel_state(())
        .await
        .expect("mullvad RPC failed")
        .into_inner();
    TunnelState::try_from(state).unwrap()
}
