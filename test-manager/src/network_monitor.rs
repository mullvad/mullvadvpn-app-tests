use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use crate::config::{HOST_NET_INTERFACE, LOCAL_WG_TUNNEL};
use futures::{
    channel::oneshot,
    future::{select, Either},
    pin_mut, StreamExt,
};
pub use pcap::Direction;
use pcap::PacketCodec;
use pnet_packet::{
    ethernet::EtherTypes,
    ip::{IpNextHeaderProtocol, IpNextHeaderProtocols},
    ipv4::Ipv4Packet,
    ipv6::Ipv6Packet,
    tcp::TcpPacket,
    udp::UdpPacket,
    Packet,
};

struct Codec {
    no_frame: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPacket {
    pub source: SocketAddr,
    pub destination: SocketAddr,
    pub protocol: IpNextHeaderProtocol,
}

impl PacketCodec for Codec {
    type Item = Option<ParsedPacket>;

    fn decode(&mut self, packet: pcap::Packet) -> Self::Item {
        if self.no_frame {
            let ip_version = (packet.data[0] & 0xf0) >> 4;

            return match ip_version {
                4 => Self::parse_ipv4(packet.data),
                6 => Self::parse_ipv6(packet.data),
                version => {
                    log::debug!("Ignoring unknown IP version: {version}");
                    None
                }
            };
        }

        let frame = pnet_packet::ethernet::EthernetPacket::new(packet.data).or_else(|| {
            log::error!("Received invalid ethernet frame");
            None
        })?;

        match frame.get_ethertype() {
            EtherTypes::Ipv4 => Self::parse_ipv4(frame.payload()),
            EtherTypes::Ipv6 => Self::parse_ipv6(frame.payload()),
            ethertype => {
                log::debug!("Ignoring unknown ethertype: {ethertype}");
                None
            }
        }
    }
}

impl Codec {
    fn parse_ipv4(payload: &[u8]) -> Option<ParsedPacket> {
        let packet = Ipv4Packet::new(payload).or_else(|| {
            log::error!("invalid v4 packet");
            None
        })?;

        let mut source = SocketAddr::new(IpAddr::V4(packet.get_source()), 0);
        let mut destination = SocketAddr::new(IpAddr::V4(packet.get_destination()), 0);

        let protocol = packet.get_next_level_protocol();

        match protocol {
            IpNextHeaderProtocols::Tcp => {
                let seg = TcpPacket::new(packet.payload()).or_else(|| {
                    log::error!("invalid TCP segment");
                    None
                })?;
                source.set_port(seg.get_source());
                destination.set_port(seg.get_destination());
            }
            IpNextHeaderProtocols::Udp => {
                let seg = UdpPacket::new(packet.payload()).or_else(|| {
                    log::error!("invalid UDP fragment");
                    None
                })?;
                source.set_port(seg.get_source());
                destination.set_port(seg.get_destination());
            }
            IpNextHeaderProtocols::Icmp => {}
            proto => log::debug!("ignoring v4 packet, transport/protocol type {proto}"),
        }

        Some(ParsedPacket {
            source,
            destination,
            protocol,
        })
    }

    fn parse_ipv6(payload: &[u8]) -> Option<ParsedPacket> {
        let packet = Ipv6Packet::new(payload).or_else(|| {
            log::error!("invalid v6 packet");
            None
        })?;

        let mut source = SocketAddr::new(IpAddr::V6(packet.get_source()), 0);
        let mut destination = SocketAddr::new(IpAddr::V6(packet.get_destination()), 0);

        let protocol = packet.get_next_header();
        match protocol {
            IpNextHeaderProtocols::Tcp => {
                let seg = TcpPacket::new(packet.payload()).or_else(|| {
                    log::error!("invalid TCP segment");
                    None
                })?;
                source.set_port(seg.get_source());
                destination.set_port(seg.get_destination());
            }
            IpNextHeaderProtocols::Udp => {
                let seg = UdpPacket::new(packet.payload()).or_else(|| {
                    log::error!("invalid UDP fragment");
                    None
                })?;
                source.set_port(seg.get_source());
                destination.set_port(seg.get_destination());
            }
            IpNextHeaderProtocols::Icmpv6 => {}
            proto => log::debug!("ignoring v6 packet, transport/protocol type {proto}"),
        }

        Some(ParsedPacket {
            source,
            destination,
            protocol,
        })
    }
}

#[derive(Debug)]
pub struct MonitorUnexpectedlyStopped(());

pub struct PacketMonitor {
    handle: tokio::task::JoinHandle<Result<MonitorResult, MonitorUnexpectedlyStopped>>,
    stop_tx: oneshot::Sender<()>,
}

pub struct MonitorResult {
    pub packets: Vec<ParsedPacket>,
    pub discarded_packets: usize,
}

impl PacketMonitor {
    /// Stop monitoring and return the result.
    pub async fn into_result(self) -> Result<MonitorResult, MonitorUnexpectedlyStopped> {
        let _ = self.stop_tx.send(());
        self.handle.await.expect("monitor panicked")
    }

    /// Wait for monitor to stop on its own.
    pub async fn wait(self) -> Result<MonitorResult, MonitorUnexpectedlyStopped> {
        self.handle.await.expect("monitor panicked")
    }
}

#[derive(Default)]
pub struct MonitorOptions {
    pub stop_on_match: bool,
    pub stop_on_non_match: bool,
    pub timeout: Option<Duration>,
    pub direction: Option<Direction>,
    pub no_frame: bool,
}

pub fn start_packet_monitor(
    filter_fn: impl Fn(&ParsedPacket) -> bool + Send + 'static,
    monitor_options: MonitorOptions,
) -> PacketMonitor {
    start_packet_monitor_for_interface(HOST_NET_INTERFACE.as_str(), filter_fn, monitor_options)
}

pub fn start_tunnel_packet_monitor(
    filter_fn: impl Fn(&ParsedPacket) -> bool + Send + 'static,
    mut monitor_options: MonitorOptions,
) -> PacketMonitor {
    monitor_options.no_frame = true;
    start_packet_monitor_for_interface(LOCAL_WG_TUNNEL, filter_fn, monitor_options)
}

fn start_packet_monitor_for_interface(
    interface: &str,
    filter_fn: impl Fn(&ParsedPacket) -> bool + Send + 'static,
    monitor_options: MonitorOptions,
) -> PacketMonitor {
    let dev = pcap::Capture::from_device(interface)
        .expect("Failed to open capture handle")
        .immediate_mode(true)
        .open()
        .expect("Failed to activate capture");

    if let Some(direction) = monitor_options.direction {
        dev.direction(direction).unwrap();
    }

    let dev = dev.setnonblock().unwrap();

    let packet_stream = dev
        .stream(Codec {
            no_frame: monitor_options.no_frame,
        })
        .unwrap();
    let (stop_tx, stop_rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let mut monitor_result = MonitorResult {
            packets: vec![],
            discarded_packets: 0,
        };
        let mut packet_stream = packet_stream.fuse();

        let timeout = async move {
            if let Some(timeout) = monitor_options.timeout {
                tokio::time::sleep(timeout).await
            } else {
                futures::future::pending().await
            }
        };

        pin_mut!(timeout);
        pin_mut!(stop_rx);

        loop {
            let next_packet = packet_stream.next();

            match select(select(next_packet, &mut stop_rx), &mut timeout).await {
                Either::Left((Either::Left((Some(Ok(packet)), _)), _)) => {
                    if let Some(packet) = packet {
                        if !filter_fn(&packet) {
                            log::debug!("\"{packet:?}\" does not match closure conditions");
                            monitor_result.discarded_packets =
                                monitor_result.discarded_packets.saturating_add(1);

                            if monitor_options.stop_on_non_match {
                                break Ok(monitor_result);
                            }
                        } else {
                            log::debug!("\"{packet:?}\" matches closure conditions");
                            monitor_result.packets.push(packet);
                            if monitor_options.stop_on_match {
                                break Ok(monitor_result);
                            }
                        }
                    }
                }
                Either::Left((Either::Left(_), _)) => {
                    log::error!("lost packet stream");
                    break Err(MonitorUnexpectedlyStopped(()));
                }
                Either::Left((Either::Right(_), _)) => {
                    log::trace!("stopping packet monitor");
                    break Ok(monitor_result);
                }
                Either::Right(_) => {
                    log::info!("monitor timed out");
                    break Ok(monitor_result);
                }
            }
        }
    });

    PacketMonitor { stop_tx, handle }
}
