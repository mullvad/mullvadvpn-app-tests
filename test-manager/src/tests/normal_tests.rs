//! This module contains the normal tests that have no special priority
use crate::assert_tunnel_state;
use super::{Error, helpers::*};

use crate::network_monitor::{start_packet_monitor, MonitorOptions};
use mullvad_management_interface::{types, ManagementServiceClient};
use mullvad_types::relay_constraints::TransportPort;
use mullvad_types::CustomTunnelEndpoint;
use mullvad_types::{
    relay_constraints::{
        Constraint, LocationConstraint, OpenVpnConstraints, RelayConstraintsUpdate,
        RelaySettingsUpdate, WireguardConstraints,
    },
    states::TunnelState,
};
use pnet_packet::ip::IpNextHeaderProtocols;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use talpid_types::net::{
    Endpoint, TransportProtocol, TunnelEndpoint, TunnelType,
};
use tarpc::context;
use test_macro::test_function;
use test_rpc::{
    Interface, ServiceClient,
};

/// Verify that outgoing TCP, UDP, and ICMP packets can be observed
/// in the disconnected state. The purpose is mostly to rule prevent
/// false negatives in other tests.
#[test_function]
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

/// Try to produce leaks in the connecting state by forcing
/// the app into the connecting state and trying to leak,
/// failing if any the following outbound traffic is
/// detected:
///
/// * TCP on port 53 and one other port
/// * UDP on port 53 and one other port
/// * ICMP (by pinging)
///
/// # Limitations
///
/// These tests are performed on one single public IP address
/// and one private IP address. They detect basic leaks but
/// do not guarantee close conformity with the security
/// document.
#[test_function]
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

/// Try to produce leaks in the error state. Refer to the
/// `test_connecting_state` documentation for details.
#[test_function]
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

    mullvad_client
        .set_allow_lan(false)
        .await
        .expect("failed to disable LAN sharing");

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

/// Connect to a single relay and verify that:
/// * Traffic can be sent and received in the tunnel.
///   This is done by pinging a single public IP address
///   and failing if there is no response.
/// * The correct relay is used.
/// * Leaks outside the tunnel are blocked. Refer to the
///   `test_connecting_state` documentation for details.
#[test_function]
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
#[test_function]
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
/// WARNING: This test will fail if host has something bound to port 53 such as a connected Mullvad
#[test_function]
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

        let connection_result = connect_and_wait(&mut mullvad_client).await;
        assert_eq!(
            connection_result.is_ok(),
            should_succeed,
            "unexpected result for port {port}: {connection_result:?}",
        );

        disconnect_and_wait(&mut mullvad_client).await?;
    }

    Ok(())
}

/// Use udp2tcp obfuscation. This test connects to a
/// WireGuard relay over TCP. It fails if no outgoing TCP
/// traffic to the relay is observed on the expected port.
#[test_function]
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
#[test_function]
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

/// Verify that traffic to private IPs is blocked when
/// "local network sharing" is disabled, but not blocked
/// when it is enabled.
/// It only checks whether outgoing UDP, TCP, and ICMP is
/// blocked for a single arbitrary private IP and port.
#[test_function]
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

/// Test whether WireGuard multihop works. This fails if:
/// * No outgoing traffic to the entry relay is
///   observed from the SUT.
/// * The conncheck reports an unexpected exit relay.
#[test_function]
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

/// Enable lockdown mode. This test succeeds if:
///
/// * Disconnected state: Outgoing traffic leaks (UDP/TCP/ICMP)
///   cannot be produced.
/// * Disconnected state: Outgoing traffic to a single
///   private IP can be produced, if and only if LAN
///   sharing is enabled.
/// * Connected state: Outgoing traffic leaks (UDP/TCP/ICMP)
///   cannot be produced.
///
/// # Limitations
///
/// These tests are performed on one single public IP address
/// and one private IP address. They detect basic leaks but
/// do not guarantee close conformity with the security
/// document.
#[test_function]
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
