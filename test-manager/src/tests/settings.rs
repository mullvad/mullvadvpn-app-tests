use super::helpers::{
    connect_and_wait, disconnect_and_wait, get_tunnel_state, ping_with_timeout, send_guest_probes,
    update_relay_settings,
};
use super::Error;
use crate::assert_tunnel_state;

use crate::network_monitor::{start_packet_monitor, MonitorOptions};
use mullvad_management_interface::ManagementServiceClient;
use mullvad_types::{
    relay_constraints::{
        Constraint, LocationConstraint, RelayConstraintsUpdate, RelaySettingsUpdate,
        WireguardConstraints,
    },
    states::TunnelState,
};
use pnet_packet::ip::IpNextHeaderProtocols;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tarpc::context;
use test_macro::test_function;
use test_rpc::{Interface, ServiceClient};

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
