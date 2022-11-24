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

#[test_module]
pub mod manager_tests {
    use super::*;

    macro_rules! assert_tunnel_state {
        ($mullvad_client:expr, $pattern:pat) => {{
            let state = get_tunnel_state($mullvad_client).await;
            assert!(matches!(state, $pattern), "state: {:?}", state);
        }};
    }

    #[manager_test(priority = -5)]
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

    #[manager_test(priority = -4)]
    pub async fn test_upgrade_app(rpc: ServiceClient) -> Result<(), Error> {
        // verify that daemon is running
        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::Running {
            return Err(Error::DaemonNotRunning);
        }

        // give it some time to start
        tokio::time::sleep(Duration::from_secs(3)).await;

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

        // TODO: check version

        Ok(())
    }

    #[manager_test(priority = -3)]
    pub async fn test_uninstall_app(rpc: ServiceClient) -> Result<(), Error> {
        // FIXME: Make it possible to perform a complete silent uninstall on Windows.
        //        Or interact with dialogs.

        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::Running {
            return Err(Error::DaemonNotRunning);
        }

        let mut ctx = context::current();
        ctx.deadline = SystemTime::now().checked_add(INSTALL_TIMEOUT).unwrap();

        rpc.uninstall_app(ctx)
            .await?
            .map_err(|error| Error::Package("uninstall app", error))?;

        // TODO: Verify that all traces of the app were removed:
        // * all program files
        // * all other files and directories, including logs, electron data, etc.
        // * devices and drivers
        // * temporary files

        if rpc.mullvad_daemon_get_status(context::current()).await? != ServiceStatus::NotRunning {
            return Err(Error::DaemonRunning);
        }

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
        // Verify that endpoint relay was selected
        //
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

        mullvad_client.connect_tunnel(()).await.map_err(|error| {
            Error::DaemonError(format!("failed to begin connecting: {}", error))
        })?;

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

    async fn disconnect_and_wait(
        mullvad_client: &mut ManagementServiceClient,
    ) -> Result<(), Error> {
        log::info!("Disconnecting");

        mullvad_client
            .disconnect_tunnel(())
            .await
            .map_err(|error| {
                Error::DaemonError(format!("failed to begin disconnecting: {}", error))
            })?;
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
                            providers: constraints.providers.map(|constraint| {
                                types::ProviderUpdate {
                                    providers: constraint
                                        .map(|providers| providers.into_vec())
                                        .unwrap_or(vec![]),
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
            .map_err(|error| {
                Error::DaemonError(format!("Failed to set relay settings: {}", error))
            })?;
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
}
