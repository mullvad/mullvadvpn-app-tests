use std::net::SocketAddr;

use mullvad_management_interface::ManagementServiceClient;
use mullvad_types::{
    relay_constraints::RelaySettingsUpdate, ConnectionConfig, CustomTunnelEndpoint,
};
use talpid_types::net::wireguard;
use test_macro::test_function;
use test_rpc::{Interface, ServiceClient};

use super::{Error, helpers::connect_and_wait};
use crate::network_monitor::{start_packet_monitor, start_tunnel_packet_monitor, MonitorOptions, Direction};

use super::helpers::update_relay_settings;

/// Test whether DNS leaks can be produced when using the default resolver. It does this by
/// connecting to a custom WireGuard relay on localhost and monitoring outbound DNS traffic in (and
/// outside of) the tunnel interface.
///
/// The test succeeds if and only if all expected outbound packets on port 53 are observed.
///
/// # Limitations
///
/// This test only detects outbound DNS leaks in the connected state.
#[test_function]
pub async fn test_dns_leak_default(
    rpc: ServiceClient,
    mullvad_client: ManagementServiceClient,
) -> Result<(), Error> {
    //
    // Connect to local wireguard relay
    //

    connect_local_wg_relay(mullvad_client.clone())
        .await
        .expect("failed to connect to custom wg relay");

    let guest_ip = rpc
        .get_interface_ip(Interface::NonTunnel)
        .await
        .expect("failed to obtain guest IP");
    let tunnel_ip = rpc
        .get_interface_ip(Interface::Tunnel)
        .await
        .expect("failed to obtain tunnel IP");

    log::debug!("Tunnel (guest) IP: {tunnel_ip}");
    log::debug!("Non-tunnel (guest) IP: {guest_ip}");

    //
    // Spoof DNS packets
    //

    let tun_bind_addr = SocketAddr::new(tunnel_ip, 0);
    let guest_bind_addr = SocketAddr::new(guest_ip, 0);

    let whitelisted_dest = "192.168.15.1:53".parse().unwrap();
    let blocked_dest_local = "10.64.100.100:53".parse().unwrap();
    let blocked_dest_public = "1.3.3.7:53".parse().unwrap();

    // Capture all outgoing DNS
    let tunnel_monitor = start_tunnel_packet_monitor(
        move |packet| packet.destination.port() == 53,
        MonitorOptions {
            direction: Some(Direction::In),
            ..Default::default()
        },
    );
    let non_tunnel_monitor = start_packet_monitor(
        move |packet| packet.destination.port() == 53,
        MonitorOptions {
            direction: Some(Direction::In),
            ..Default::default()
        },
    );
    // Using the default resolver, we should observe 2 outgoing packets to the
    // whitelisted destination on port 53, and only inside the tunnel.

    spoof_packets(&rpc, tun_bind_addr, whitelisted_dest);
    spoof_packets(&rpc, guest_bind_addr, whitelisted_dest);

    spoof_packets(&rpc, tun_bind_addr, blocked_dest_local);
    spoof_packets(&rpc, guest_bind_addr, blocked_dest_local);

    spoof_packets(&rpc, tun_bind_addr, blocked_dest_public);
    spoof_packets(&rpc, guest_bind_addr, blocked_dest_public);

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    //
    // Examine tunnel traffic
    //

    let tunnel_result = tunnel_monitor.into_result().await.unwrap();
    assert!(
        tunnel_result.packets.len() >= 2,
        "expected at least in-tunnel 2 packets to allowed destination only"
    );

    for pkt in tunnel_result.packets {
        assert_eq!(
            pkt.destination, whitelisted_dest,
            "unexpected tunnel packet on port 53"
        );
    }

    //
    // Examine non-tunnel traffic
    //

    let non_tunnel_result = non_tunnel_monitor.into_result().await.unwrap();
    assert_eq!(
        non_tunnel_result.packets.len(),
        0,
        "expected no non-tunnel packets on port 53"
    );

    Ok(())
}

/// Connect to the WireGuard relay that is set up in scripts/setup-network.sh
/// See that script for details.
async fn connect_local_wg_relay(mut mullvad_client: ManagementServiceClient) -> Result<(), Error> {
    let peer_addr: SocketAddr = "172.29.1.200:51820".parse().unwrap();

    let tun_self_addr: Ipv4Addr = Ipv4Addr::new(192, 168, 15, 2);
    let tun_peer_addr: Ipv4Addr = Ipv4Addr::new(192, 168, 15, 1);

    let public_key =
        wireguard::PublicKey::from_base64("7svBwGBefP7KVmH/yes+pZCfO6uSOYeGieYYa1+kZ0E=").unwrap();
    let private_key = wireguard::PrivateKey::from(
        TryInto::<[u8; 32]>::try_into(
            base64::decode("mPue6Xt0pdz4NRAhfQSp/SLKo7kV7DW+2zvBq0N9iUI=").unwrap(),
        )
        .unwrap(),
    );

    let relay_settings = RelaySettingsUpdate::CustomTunnelEndpoint(CustomTunnelEndpoint {
        host: peer_addr.ip().to_string(),
        config: ConnectionConfig::Wireguard(wireguard::ConnectionConfig {
            tunnel: wireguard::TunnelConfig {
                addresses: vec![IpAddr::V4(tun_self_addr)],
                private_key,
            },
            peer: wireguard::PeerConfig {
                public_key,
                allowed_ips: vec!["0.0.0.0/0".parse().unwrap()],
                endpoint: peer_addr,
                psk: None,
            },
            ipv4_gateway: tun_peer_addr,
            exit_peer: None,
            fwmark: None,
            ipv6_gateway: None,
        }),
    });

    update_relay_settings(&mut mullvad_client, relay_settings)
        .await
        .expect("failed to update relay settings");

    connect_and_wait(&mut mullvad_client).await?;

    Ok(())
}

fn spoof_packets(rpc: &ServiceClient, bind_addr: SocketAddr, dest: SocketAddr) {
    let rpc1 = rpc.clone();
    let rpc2 = rpc.clone();
    tokio::spawn(async move {
        let _ = rpc1.send_tcp(bind_addr, dest).await;
    });
    tokio::spawn(async move {
        let _ = rpc2.send_udp(bind_addr, dest).await;
    });
}
