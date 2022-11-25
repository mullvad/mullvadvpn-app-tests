mod test_metadata;

use crate::network_monitor::{start_packet_monitor, MonitorOptions};
use mullvad_management_interface::{
    types::{self, RelayLocation},
    ManagementServiceClient,
};
use mullvad_types::{
    relay_constraints::{
        Constraint, LocationConstraint, OpenVpnConstraints, Ownership, RelayConstraintsUpdate,
        RelaySettingsUpdate, WireguardConstraints,
    },
    states::TunnelState,
};
use pnet_packet::ip::IpNextHeaderProtocols;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    time::{Duration, SystemTime},
};
use talpid_types::net::{
    wireguard::{PeerConfig, PrivateKey, TunnelConfig},
    Endpoint, IpVersion, TransportProtocol, TunnelEndpoint, TunnelType,
};
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
    use mullvad_types::CustomTunnelEndpoint;

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
        let inet_destination: SocketAddr = "1.1.1.1:1337".parse().unwrap();
        let bind_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();

        // Give it some time to start
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify that daemon is running
        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::Running {
            return Err(Error::DaemonNotRunning);
        }

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
            loop {
                let _ = ping_rpc
                    .send_tcp(context::current(), bind_addr, inet_destination)
                    .await;
                let _ = ping_rpc
                    .send_udp(context::current(), bind_addr, inet_destination)
                    .await;
                let _ = ping_with_timeout(&ping_rpc, inet_destination.ip(), None).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }));

        // install new package
        let mut ctx = context::current();
        ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

        rpc.install_app(ctx, get_package_desc(&rpc, "current-app").await?)
            .await?
            .map_err(|error| Error::Package("current app", error))?;

        // Give it some time to start
        tokio::time::sleep(Duration::from_secs(3)).await;

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
            monitor_result.packets.len(),
            0,
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
    pub async fn test_disconnected_state(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        let inet_destination = "1.3.3.7:1337".parse().unwrap();

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        //
        // Test whether outgoing packets can be observed
        //

        log::info!("Sending packets to {inet_destination}");

        let detected_probes =
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), inet_destination).await?;
        assert!(
            detected_probes.all(),
            "did not see (all) outgoing packets to destination: {detected_probes:?}",
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
    pub async fn test_connecting_state(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        let inet_destination = "1.1.1.1:1337".parse().unwrap();
        let lan_destination = "172.29.1.200:53".parse().unwrap();
        let inet_dns = "1.1.1.1:53".parse().unwrap();
        let lan_dns = "172.29.1.200:53".parse().unwrap();

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        let relay_settings = RelaySettingsUpdate::CustomTunnelEndpoint(CustomTunnelEndpoint {
            host: "1.3.3.7".to_owned(),
            config: mullvad_types::ConnectionConfig::Wireguard(unreachable_wireguard_tunnel()),
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to update relay settings");

        mullvad_client
            .connect_tunnel(())
            .await
            .expect("failed to begin connecting");
        let new_state = wait_for_tunnel_state(mullvad_client.clone(), |state| {
            matches!(
                state,
                TunnelState::Connecting { .. } | TunnelState::Error(..)
            )
        })
        .await?;

        assert!(
            matches!(new_state, TunnelState::Connecting { .. }),
            "failed to enter connecting state: {:?}",
            new_state
        );

        //
        // Leak test
        //

        assert!(
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), inet_destination)
                .await?
                .none(),
            "observed unexpected outgoing packets (inet)"
        );
        assert!(
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), lan_destination)
                .await?
                .none(),
            "observed unexpected outgoing packets (lan)"
        );
        assert!(
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), inet_dns)
                .await?
                .none(),
            "observed unexpected outgoing packets (DNS, inet)"
        );
        assert!(
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), lan_dns)
                .await?
                .none(),
            "observed unexpected outgoing packets (DNS, lan)"
        );

        assert_tunnel_state!(&mut mullvad_client, TunnelState::Connecting { .. });

        //
        // Disconnect
        //

        log::info!("Disconnecting");

        disconnect_and_wait(&mut mullvad_client).await?;

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            location: Some(Constraint::Any),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to update relay settings");

        Ok(())
    }

    #[manager_test]
    pub async fn test_error_state(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        let inet_destination = "1.1.1.1:1337".parse().unwrap();
        let lan_destination = "172.29.1.200:53".parse().unwrap();
        let inet_dns = "1.1.1.1:53".parse().unwrap();
        let lan_dns = "172.29.1.200:53".parse().unwrap();

        log::info!("Verify tunnel state: disconnected");
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

        //
        // Connect to non-existent location
        //

        log::info!("Enter error state");

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            location: Some(Constraint::Only(LocationConstraint::Country(
                "xx".to_string(),
            ))),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to update relay settings");

        let _ = connect_and_wait(&mut mullvad_client).await;
        assert_tunnel_state!(&mut mullvad_client, TunnelState::Error { .. });

        //
        // Leak test
        //

        assert!(
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), inet_destination)
                .await?
                .none(),
            "observed unexpected outgoing packets (inet)"
        );
        assert!(
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), lan_destination)
                .await?
                .none(),
            "observed unexpected outgoing packets (lan)"
        );
        assert!(
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), inet_dns)
                .await?
                .none(),
            "observed unexpected outgoing packets (DNS, inet)"
        );
        assert!(
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), lan_dns)
                .await?
                .none(),
            "observed unexpected outgoing packets (DNS, lan)"
        );

        //
        // Disconnect
        //

        log::info!("Disconnecting");

        disconnect_and_wait(&mut mullvad_client).await?;

        let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {
            location: Some(Constraint::Any),
            ..Default::default()
        });

        update_relay_settings(&mut mullvad_client, relay_settings)
            .await
            .expect("failed to update relay settings");

        Ok(())
    }

    #[manager_test]
    pub async fn test_connected_state(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        let inet_destination = "1.1.1.1:1337".parse().unwrap();

        reset_relay_settings(&mut mullvad_client).await?;

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

        log::info!("Test whether outgoing non-tunnel traffic is blocked");

        let detected_probes =
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), inet_destination).await?;
        assert!(
            detected_probes.none(),
            "observed unexpected outgoing packets"
        );

        //
        // Ping inside tunnel while connected
        //

        log::info!("Ping inside tunnel");

        ping_with_timeout(&rpc, inet_destination.ip(), Some(Interface::Tunnel))
            .await
            .unwrap();

        disconnect_and_wait(&mut mullvad_client).await?;

        Ok(())
    }

    /// Set up an OpenVPN tunnel, UDP as well as TCP.
    /// This test fails if a working tunnel cannot be set up.
    #[manager_test]
    pub async fn test_openvpn_tunnel(
        _rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        // TODO: observe traffic on the expected destination/port (only)

        reset_relay_settings(&mut mullvad_client).await?;

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

        Ok(())
    }

    /// Set up a WireGuard tunnel.
    /// This test fails if a working tunnel cannot be set up.
    #[manager_test]
    pub async fn test_wireguard_tunnel(
        _rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        // TODO: observe UDP traffic on the expected destination/port (only)
        // TODO: IPv6

        reset_relay_settings(&mut mullvad_client).await?;

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

        Ok(())
    }

    /// Use udp2tcp obfuscation. This test connects to a
    /// WireGuard relay over TCP. It fails if no outgoing TCP
    /// traffic to the relay is observed on the expected port.
    #[manager_test]
    pub async fn test_udp2tcp_tunnel(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        // TODO: check if src <-> target / tcp is observed (only)
        // TODO: ping a public IP on the fake network (not possible using real relay)
        const PING_DESTINATION: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

        reset_relay_settings(&mut mullvad_client).await?;

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
        assert_eq!(monitor_result.discarded_packets, 0);

        disconnect_and_wait(&mut mullvad_client).await?;

        Ok(())
    }

    /// Test whether bridge mode works. This fails if:
    /// * No outgoing traffic to the bridge/entry relay is
    ///   observed from the SUT.
    /// * The conncheck reports an unexpected exit relay.
    #[manager_test]
    pub async fn test_bridge(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        const EXPECTED_EXIT_HOSTNAME: &str = "se-got-006";
        const EXPECTED_ENTRY_IP: Ipv4Addr = Ipv4Addr::new(185, 213, 154, 117);

        reset_relay_settings(&mut mullvad_client).await?;

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
            monitor_result.packets.len() > 0,
            "detected no traffic to entry server",
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
    pub async fn test_lan(
        rpc: ServiceClient,
        mut mullvad_client: ManagementServiceClient,
    ) -> Result<(), Error> {
        let lan_destination = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 29, 1, 200)), 1234);

        reset_relay_settings(&mut mullvad_client).await?;

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

        log::info!("Test whether outgoing LAN traffic is blocked");

        let detected_probes =
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), lan_destination).await?;
        assert!(
            detected_probes.none(),
            "observed unexpected outgoing LAN packets"
        );

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

        log::info!("Test whether outgoing LAN traffic is blocked");

        let detected_probes =
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), lan_destination).await?;
        assert!(
            detected_probes.all(),
            "did not observe all outgoing LAN packets"
        );

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

        reset_relay_settings(&mut mullvad_client).await?;

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
        assert!(monitor_result.packets.len() > 0, "no matching packets",);

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
        let lan_destination: SocketAddr = "172.29.1.200:1337".parse().unwrap();
        let inet_destination: SocketAddr = "1.1.1.1:1337".parse().unwrap();

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

        let detected_probes =
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), lan_destination).await?;
        assert!(detected_probes.none(), "observed outgoing packets to LAN");

        let detected_probes =
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), inet_destination).await?;
        assert!(
            detected_probes.none(),
            "observed outgoing packets to internet"
        );

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

        let detected_probes =
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), lan_destination).await?;
        assert!(
            detected_probes.all(),
            "did not observe some outgoing packets"
        );

        let detected_probes =
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), inet_destination).await?;
        assert!(
            detected_probes.none(),
            "observed outgoing packets to internet"
        );

        //
        // Connect
        //

        connect_and_wait(&mut mullvad_client).await?;

        //
        // Leak test
        //

        ping_with_timeout(&rpc, inet_destination.ip(), Some(Interface::Tunnel))
            .await
            .expect("Failed to ping internet target");

        let detected_probes =
            send_guest_probes(rpc.clone(), Some(Interface::NonTunnel), inet_destination).await?;
        assert!(
            detected_probes.none(),
            "observed outgoing packets to internet"
        );

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

#[derive(Debug, Default)]
struct ProbeResult {
    tcp: usize,
    udp: usize,
    icmp: usize,
}

impl ProbeResult {
    pub fn all(&self) -> bool {
        self.tcp > 0 && self.udp > 0 && self.icmp > 0
    }

    pub fn none(&self) -> bool {
        !self.any()
    }

    pub fn any(&self) -> bool {
        self.tcp > 0 || self.udp > 0 || self.icmp > 0
    }
}

/// Sends a number of probes and returns the number of observed packets (UDP, TCP, or ICMP)
async fn send_guest_probes(
    rpc: ServiceClient,
    interface: Option<Interface>,
    destination: SocketAddr,
) -> Result<ProbeResult, Error> {
    let pktmon = start_packet_monitor(
        move |packet| packet.destination.ip() == destination.ip(),
        MonitorOptions {
            direction: Some(crate::network_monitor::Direction::In),
            timeout: Some(Duration::from_secs(3)),
            ..Default::default()
        },
    );

    let bind_addr = if let Some(interface) = interface {
        SocketAddr::new(
            rpc.get_interface_ip(context::current(), interface)
                .await?
                .expect("failed to obtain interface IP"),
            0,
        )
    } else {
        "0.0.0.0:0".parse().unwrap()
    };

    let send_handle = tokio::spawn(async move {
        let tcp_rpc = rpc.clone();
        let udp_rpc = rpc.clone();
        tokio::spawn(async move {
            let _ = tcp_rpc
                .send_tcp(context::current(), bind_addr, destination)
                .await;
        });
        tokio::spawn(async move {
            let _ = udp_rpc
                .send_udp(context::current(), bind_addr, destination)
                .await;
        });
        let _ = ping_with_timeout(&rpc, destination.ip(), interface).await?;
        Ok::<(), Error>(())
    });

    let monitor_result = pktmon.wait().await.unwrap();

    send_handle.abort();

    let mut result = ProbeResult::default();

    for pkt in monitor_result.packets {
        match pkt.protocol {
            IpNextHeaderProtocols::Tcp => {
                result.tcp = result.tcp.saturating_add(1);
            }
            IpNextHeaderProtocols::Udp => {
                result.udp = result.udp.saturating_add(1);
            }
            IpNextHeaderProtocols::Icmp => {
                result.icmp = result.icmp.saturating_add(1);
            }
            _ => (),
        }
    }

    Ok(result)
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

/// Disconnect and reset all relay, bridge, and obfuscation settings.
async fn reset_relay_settings(
    mullvad_client: &mut ManagementServiceClient,
) -> Result<(), Error> {
    disconnect_and_wait(mullvad_client).await?;

    let relay_settings = RelaySettingsUpdate::Normal(RelayConstraintsUpdate {

        location: Some(Constraint::Only(LocationConstraint::Country(
            "se".to_string(),
        ))),
        tunnel_protocol: Some(Constraint::Any),
        openvpn_constraints: Some(OpenVpnConstraints::default()),
        wireguard_constraints: Some(WireguardConstraints::default()),
        ..Default::default()
    });

    update_relay_settings(mullvad_client, relay_settings)
        .await
        .map_err(|error| Error::DaemonError(format!("Failed to reset relay settings: {}", error)))?;

    mullvad_client
        .set_bridge_state(types::BridgeState {
            state: i32::from(types::bridge_state::State::Auto),
        })
        .await
        .map_err(|error| Error::DaemonError(format!("Failed to reset bridge mode: {}", error)))?;

    mullvad_client
        .set_obfuscation_settings(types::ObfuscationSettings {
            selected_obfuscation: i32::from(
                types::obfuscation_settings::SelectedObfuscation::Off,
            ),
            udp2tcp: Some(types::Udp2TcpObfuscationSettings { port: 0 }),
        })
        .await
        .map(|_| ())
        .map_err(|error| Error::DaemonError(format!("Failed to reset obfuscation: {}", error)))
}

async fn update_relay_settings(
    mullvad_client: &mut ManagementServiceClient,
    relay_settings_update: RelaySettingsUpdate,
) -> Result<(), Error> {
    // TODO: use From implementation in mullvad_management_interface

    let update = match relay_settings_update {
        RelaySettingsUpdate::Normal(constraints) => types::RelaySettingsUpdate {
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
                    ownership: constraints
                        .ownership
                        .map(|ownership| types::OwnershipUpdate {
                            ownership: i32::from(match ownership.as_ref() {
                                Constraint::Any => types::Ownership::Any,
                                Constraint::Only(ownership) => match ownership {
                                    Ownership::MullvadOwned => types::Ownership::MullvadOwned,
                                    Ownership::Rented => types::Ownership::Rented,
                                },
                            }),
                        }),
                    tunnel_type: constraints.tunnel_protocol.map(|protocol| {
                        types::TunnelTypeUpdate {
                            tunnel_type: match protocol {
                                Constraint::Any => None,
                                Constraint::Only(protocol) => Some(types::TunnelTypeConstraint {
                                    tunnel_type: i32::from(match protocol {
                                        TunnelType::Wireguard => types::TunnelType::Wireguard,
                                        TunnelType::OpenVpn => types::TunnelType::Openvpn,
                                    }),
                                }),
                            },
                        }
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
                    openvpn_constraints: constraints.openvpn_constraints.map(
                        |openvpn_constraints| types::OpenvpnConstraints {
                            port: openvpn_constraints
                                .port
                                .option()
                                .map(types::TransportPort::from),
                        },
                    ),
                },
            )),
        },
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

fn unreachable_wireguard_tunnel() -> talpid_types::net::wireguard::ConnectionConfig {
    talpid_types::net::wireguard::ConnectionConfig {
        tunnel: TunnelConfig {
            private_key: PrivateKey::new_from_random(),
            addresses: vec![IpAddr::V4(Ipv4Addr::new(10, 64, 10, 1))],
        },
        peer: PeerConfig {
            public_key: PrivateKey::new_from_random().public_key(),
            allowed_ips: all_of_the_internet(),
            endpoint: "1.3.3.7:1234".parse().unwrap(),
            psk: None,
        },
        exit_peer: None,
        ipv4_gateway: Ipv4Addr::new(10, 64, 10, 1),
        ipv6_gateway: None,
        #[cfg(target_os = "linux")]
        fwmark: None,
    }
}

pub fn all_of_the_internet() -> Vec<ipnetwork::IpNetwork> {
    vec![
        "0.0.0.0/0".parse().expect("Failed to parse ipv6 network"),
        "::0/0".parse().expect("Failed to parse ipv6 network"),
    ]
}
