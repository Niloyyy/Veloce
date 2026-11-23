use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};

use core_protocol::{NodeId, PROTOCOL_VERSION};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::error::{Result, TransportError};

/// DNS-SD service type advertised on the local subnet.
pub const MDNS_SERVICE_TYPE: &str = "_veloce._udp.local.";

/// Metadata for a Veloce peer discovered on the local network via mDNS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPeer {
    pub node_id: NodeId,
    pub device_name: String,
    pub addresses: Vec<SocketAddr>,
    pub port: u16,
    pub protocol_version: u16,
}

/// Events emitted by the local mDNS discovery subsystem.
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    PeerDiscovered(LocalPeer),
    PeerLost(NodeId),
}

/// Classifies a network interface and IP address for LAN advertisement priority.
///
/// Returns `None` for non-routable/excluded interfaces (loopback, link-local, container/VM
/// bridges like `docker0`, `veth*`, `br-*`, `virbr*`, `vmnet*`, `vboxnet*`), `Some(0)` for
/// physical private LAN IPv4 NICs, `Some(1)` for physical non-private IPv4, `Some(2)` for
/// physical IPv6, and `Some(3)` for VPN/TUN adapters (only used as a deprioritized fallback).
pub fn classify_interface_priority(iface_name: &str, ip: IpAddr) -> Option<u8> {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return None;
    }
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_link_local() {
                return None;
            }
        }
        IpAddr::V6(v6) => {
            // Skip link-local IPv6 (fe80::/10) which requires OS interface zone scopes.
            if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                return None;
            }
        }
    }

    let lower = iface_name.to_ascii_lowercase();

    // Always exclude container, hypervisor, and loopback virtual bridges.
    const EXCLUDED_PREFIXES: &[&str] = &[
        "lo",
        "docker",
        "br-",
        "veth",
        "virbr",
        "cni",
        "flannel",
        "calico",
        "kube",
        "podman",
        "lxc",
        "lxd",
        "vmnet",
        "vboxnet",
        "vethernet",
    ];
    if EXCLUDED_PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return None;
    }

    // Deprioritize VPN / tunnel interfaces behind physical LAN NICs.
    const VPN_PREFIXES: &[&str] = &["tun", "tap", "utun", "wg", "tailscale", "zt", "ppp"];
    let is_vpn = VPN_PREFIXES.iter().any(|p| lower.starts_with(p));
    if is_vpn {
        return Some(if ip.is_ipv4() { 3 } else { 4 });
    }

    match ip {
        IpAddr::V4(v4) if v4.is_private() => Some(0),
        IpAddr::V4(_) => Some(1),
        IpAddr::V6(_) => Some(2),
    }
}

/// Enumerates local network addresses bound to `port`, filtering out virtual container/VM bridges
/// and link-local addresses and ordering physical LAN NICs ahead of VPN/TUN fallbacks.
pub fn local_lan_addrs(port: u16) -> Vec<SocketAddr> {
    let mut ranked: Vec<(u8, SocketAddr)> = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            let ip = iface.ip();
            let Some(prio) = classify_interface_priority(&iface.name, ip) else {
                continue;
            };
            let sock = SocketAddr::new(ip, port);
            if !ranked.iter().any(|(_, existing)| *existing == sock) {
                ranked.push((prio, sock));
            }
        }
    }
    ranked.sort_by_key(|(prio, _)| *prio);

    // If real physical LAN NICs exist (priority <= 2), omit VPN/tunnel adapters from LAN hints.
    let has_physical = ranked.iter().any(|(prio, _)| *prio <= 2);
    ranked
        .into_iter()
        .filter(|(prio, _)| !has_physical || *prio <= 2)
        .map(|(_, addr)| addr)
        .collect()
}

/// Manages mDNS advertisement and zero-configuration browsing of nearby LAN peers.
pub struct LanDiscovery {
    daemon: ServiceDaemon,
    service_fullname: String,
    peers: Arc<RwLock<HashMap<NodeId, LocalPeer>>>,
    events_tx: broadcast::Sender<DiscoveryEvent>,
    cancel: CancellationToken,
}

impl LanDiscovery {
    /// Starts advertising this node over mDNS and browsing for other `_veloce._udp.local.` peers.
    pub fn start(
        local_node_id: NodeId,
        device_name: String,
        listen_port: u16,
    ) -> Result<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| TransportError::Discovery(format!("failed to start mDNS daemon: {e}")))?;

        let instance_name = format!("veloce-{}", local_node_id.short_id());
        let host_name = format!("{instance_name}.local.");
        let ips: Vec<IpAddr> = local_lan_addrs(listen_port)
            .into_iter()
            .map(|a| a.ip())
            .collect();

        let mut properties = HashMap::new();
        properties.insert("node_id".to_string(), local_node_id.to_base32());
        properties.insert("name".to_string(), device_name);
        properties.insert("ver".to_string(), PROTOCOL_VERSION.to_string());

        let service_info = ServiceInfo::new(
            MDNS_SERVICE_TYPE,
            &instance_name,
            &host_name,
            ips.as_slice(),
            listen_port,
            Some(properties),
        )
        .map_err(|e| TransportError::Discovery(format!("invalid mDNS ServiceInfo: {e}")))?;

        let service_fullname = service_info.get_fullname().to_string();
        daemon
            .register(service_info)
            .map_err(|e| TransportError::Discovery(format!("mDNS register failed: {e}")))?;

        let receiver = daemon
            .browse(MDNS_SERVICE_TYPE)
            .map_err(|e| TransportError::Discovery(format!("mDNS browse failed: {e}")))?;

        let peers: Arc<RwLock<HashMap<NodeId, LocalPeer>>> = Arc::new(RwLock::new(HashMap::new()));
        let (events_tx, _) = broadcast::channel(128);
        let cancel = CancellationToken::new();

        let peers_bg = Arc::clone(&peers);
        let tx_bg = events_tx.clone();
        let cancel_bg = cancel.clone();

        // Spawn dedicated background reader on blocking thread since mdns-sd uses flume blocking recv.
        std::thread::Builder::new()
            .name("veloce-mdns-browser".into())
            .spawn(move || {
                let mut fullname_to_node: HashMap<String, NodeId> = HashMap::new();
                while !cancel_bg.is_cancelled() {
                    match receiver.recv_timeout(std::time::Duration::from_millis(250)) {
                        Ok(ServiceEvent::ServiceResolved(info)) => {
                            let Some(node_str) = info.get_property_val_str("node_id") else {
                                continue;
                            };
                            let Ok(peer_id) = NodeId::parse(node_str) else {
                                continue;
                            };
                            if peer_id == local_node_id {
                                continue;
                            }
                            let name = info
                                .get_property_val_str("name")
                                .unwrap_or("Veloce Peer")
                                .to_string();
                            let ver = info
                                .get_property_val_str("ver")
                                .and_then(|s| s.parse::<u16>().ok())
                                .unwrap_or(PROTOCOL_VERSION);
                            let port = info.get_port();
                            let mut addresses: Vec<SocketAddr> = info
                                .get_addresses()
                                .iter()
                                .map(|ip| SocketAddr::new(*ip, port))
                                .collect();
                            addresses.sort_by_key(|a| if a.is_ipv4() { 0u8 } else { 1u8 });

                            if addresses.is_empty() {
                                continue;
                            }

                            let peer = LocalPeer {
                                node_id: peer_id,
                                device_name: name,
                                addresses,
                                port,
                                protocol_version: ver,
                            };

                            fullname_to_node.insert(info.get_fullname().to_string(), peer_id);
                            {
                                let mut map = peers_bg.write().expect("peers lock poisoned");
                                map.insert(peer_id, peer.clone());
                            }
                            let _ = tx_bg.send(DiscoveryEvent::PeerDiscovered(peer));
                        }
                        Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                            if let Some(removed_id) = fullname_to_node.remove(&fullname) {
                                {
                                    let mut map = peers_bg.write().expect("peers lock poisoned");
                                    map.remove(&removed_id);
                                }
                                let _ = tx_bg.send(DiscoveryEvent::PeerLost(removed_id));
                            }
                        }
                        Ok(_) => {}
                        Err(flume_err) => {
                            if flume_err.to_string().contains("disconnected") {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|e| TransportError::Discovery(format!("failed to spawn mDNS thread: {e}")))?;

        Ok(Self {
            daemon,
            service_fullname,
            peers,
            events_tx,
            cancel,
        })
    }

    pub fn list_peers(&self) -> Vec<LocalPeer> {
        let map = self.peers.read().expect("peers lock poisoned");
        map.values().cloned().collect()
    }

    pub fn get_peer(&self, node_id: &NodeId) -> Option<LocalPeer> {
        let map = self.peers.read().expect("peers lock poisoned");
        map.get(node_id).cloned()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DiscoveryEvent> {
        self.events_tx.subscribe()
    }

    pub fn shutdown(&self) {
        self.cancel.cancel();
        let _ = self.daemon.unregister(&self.service_fullname);
        let _ = self.daemon.shutdown();
    }
}

impl Drop for LanDiscovery {
    fn drop(&mut self) {
        self.shutdown();
    }
}
