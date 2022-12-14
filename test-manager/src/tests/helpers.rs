use super::{Error, PING_TIMEOUT, WAIT_FOR_TUNNEL_STATE_TIMEOUT};
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
    time::Duration,
};
use talpid_types::net::{
    wireguard::{PeerConfig, PrivateKey, TunnelConfig},
    IpVersion, TunnelType,
};
use tarpc::context;
use test_rpc::{meta, package::Package, AmIMullvad, Interface, ServiceClient};
use tokio::time::timeout;

#[macro_export]
macro_rules! assert_tunnel_state {
    ($mullvad_client:expr, $pattern:pat) => {{
        let state = get_tunnel_state($mullvad_client).await;
        assert!(matches!(state, $pattern), "state: {:?}", state);
    }};
}

/// Return all possible API endpoints. Note that this includes all bridge IPs. Ideally,
/// we'd keep track of the current API IP, not exonerate all bridges from being considered
/// leaky.
#[macro_export]
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

pub async fn get_package_desc(rpc: &ServiceClient, name: &str) -> Result<Package, Error> {
    match rpc.get_os(context::current()).await.map_err(Error::Rpc)? {
        meta::Os::Linux => Ok(Package {
            path: Path::new(&format!("/opt/testing/{}", name)).to_path_buf(),
        }),
        meta::Os::Windows => Ok(Package {
            path: Path::new(&format!(r"E:\{}", name)).to_path_buf(),
        }),
        _ => unimplemented!(),
    }
}

#[derive(Debug, Default)]
pub struct ProbeResult {
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
pub async fn send_guest_probes(
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
        ping_with_timeout(&rpc, destination.ip(), interface).await?;
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

pub async fn ping_with_timeout(
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

pub async fn connect_and_wait(mullvad_client: &mut ManagementServiceClient) -> Result<(), Error> {
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

pub async fn disconnect_and_wait(
    mullvad_client: &mut ManagementServiceClient,
) -> Result<(), Error> {
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

pub async fn wait_for_tunnel_state(
    rpc: mullvad_management_interface::ManagementServiceClient,
    accept_state_fn: impl Fn(&mullvad_types::states::TunnelState) -> bool,
) -> Result<mullvad_types::states::TunnelState, Error> {
    tokio::time::timeout(
        WAIT_FOR_TUNNEL_STATE_TIMEOUT,
        wait_for_tunnel_state_inner(rpc, accept_state_fn),
    )
    .await
    .map_err(|_error| Error::DaemonError(String::from("Tunnel event listener timed out")))?
}

async fn wait_for_tunnel_state_inner(
    mut rpc: mullvad_management_interface::ManagementServiceClient,
    accept_state_fn: impl Fn(&mullvad_types::states::TunnelState) -> bool,
) -> Result<mullvad_types::states::TunnelState, Error> {
    let events = rpc
        .events_listen(())
        .await
        .map_err(|status| Error::DaemonError(format!("Failed to get event stream: {}", status)))?;

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

    let mut events = events.into_inner();
    loop {
        match events.message().await {
            Ok(Some(event)) => match event.event.unwrap() {
                mullvad_management_interface::types::daemon_event::Event::TunnelState(
                    new_state,
                ) => {
                    let state = mullvad_types::states::TunnelState::try_from(new_state).map_err(
                        |error| Error::DaemonError(format!("Invalid tunnel state: {:?}", error)),
                    )?;
                    if accept_state_fn(&state) {
                        return Ok(state);
                    }
                }
                _ => continue,
            },
            Ok(None) => break Err(Error::DaemonError(String::from("Lost daemon event stream"))),
            Err(status) => {
                break Err(Error::DaemonError(format!(
                    "Failed to get next event: {}",
                    status
                )))
            }
        }
    }
}

pub async fn geoip_lookup_with_retries(rpc: ServiceClient) -> Result<AmIMullvad, Error> {
    const MAX_ATTEMPTS: usize = 5;
    const BEFORE_RETRY_DELAY: Duration = Duration::from_secs(2);

    let mut attempt = 0;

    loop {
        let result = geoip_lookup_inner(&rpc).await;

        attempt += 1;
        if result.is_ok() || attempt >= MAX_ATTEMPTS {
            return result;
        }

        tokio::time::sleep(BEFORE_RETRY_DELAY).await;
    }
}

async fn geoip_lookup_inner(rpc: &ServiceClient) -> Result<AmIMullvad, Error> {
    rpc.geoip_lookup(context::current())
        .await
        .map_err(Error::Rpc)?
        .map_err(Error::GeoipError)
}

pub struct AbortOnDrop<T>(pub tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Disconnect and reset all relay, bridge, and obfuscation settings.
pub async fn reset_relay_settings(
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
        .map_err(|error| {
            Error::DaemonError(format!("Failed to reset relay settings: {}", error))
        })?;

    mullvad_client
        .set_bridge_state(types::BridgeState {
            state: i32::from(types::bridge_state::State::Auto),
        })
        .await
        .map_err(|error| Error::DaemonError(format!("Failed to reset bridge mode: {}", error)))?;

    mullvad_client
        .set_obfuscation_settings(types::ObfuscationSettings {
            selected_obfuscation: i32::from(types::obfuscation_settings::SelectedObfuscation::Off),
            udp2tcp: Some(types::Udp2TcpObfuscationSettings { port: 0 }),
        })
        .await
        .map(|_| ())
        .map_err(|error| Error::DaemonError(format!("Failed to reset obfuscation: {}", error)))
}

#[allow(clippy::or_fun_call)]
pub async fn update_relay_settings(
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

pub async fn get_tunnel_state(mullvad_client: &mut ManagementServiceClient) -> TunnelState {
    let state = mullvad_client
        .get_tunnel_state(())
        .await
        .expect("mullvad RPC failed")
        .into_inner();
    TunnelState::try_from(state).unwrap()
}

pub fn unreachable_wireguard_tunnel() -> talpid_types::net::wireguard::ConnectionConfig {
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
