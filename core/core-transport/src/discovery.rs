use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use core_protocol::{NodeId, PROTOCOL_VERSION};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::error::{Result, TransportError};

/// DNS-SD service type advertised on the local subnet.
pub const MDNS_SERVICE_TYPE: &str = "_veloce._udp.local.";

/// Lightweight UDP LAN presence beacon port & multicast group for sub-second peer join/leave updates.
const LAN_BEACON_PORT: u16 = 53539;
const LAN_BEACON_MULTICAST: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 88);
const LAN_BEACON_MAGIC: [u8; 4] = [0x1B, b'V', b'L', b'P'];

#[derive(Debug, Clone, Serialize, Deserialize)]
enum LanBeaconPacket {
    Hello {
        node_id: NodeId,
        device_name: String,
        listen_port: u16,
        protocol_version: u16,
        ips: Vec<IpAddr>,
    },
    Goodbye {
        node_id: NodeId,
    },
}

#[derive(Debug, Clone)]
struct PeerEntry {
    peer: LocalPeer,
    last_seen: Instant,
}

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

/// Enumerates local network addresses bound to `port`, honoring explicit platform `overrides`
/// (e.g. Android `ConnectivityManager` Wi-Fi IPs on API 30+ where `RTM_GETLINK` is restricted),
/// filtering out virtual container/VM bridges and link-local addresses, and falling back to
/// `getifs::local_addrs()` if full interface table enumeration is blocked by the OS.
///
/// When `include_vpn_fallback` is `false` (used for LAN mDNS), VPN/mesh adapters are omitted
/// whenever physical LAN NICs exist. When `true` (used for global `veloce1...` tickets), mesh
/// VPN adapters (Tailscale, ZeroTier, WireGuard) are retained as lower-priority fallback candidates
/// so zero-cost mesh VPN setups work seamlessly across strict cellular CGNAT.
pub fn local_addrs_with_options(
    port: u16,
    overrides: &[IpAddr],
    include_vpn_fallback: bool,
) -> Vec<SocketAddr> {
    let mut ranked: Vec<(u8, SocketAddr)> = Vec::new();

    // Explicit platform-supplied IPs: classify by IP type so LAN IPv4 stays tier 0,
    // global IPv6 stays tier 2, and CGNAT/Tailscale VPN IPs (100.64.0.0/10) stay tier 3.
    for &ip in overrides {
        if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
            continue;
        }
        let prio = match ip {
            IpAddr::V4(v4) if v4.is_link_local() => continue,
            IpAddr::V4(v4) if is_cgnat_or_tailscale_v4(v4) => 3,
            IpAddr::V4(v4) if v4.is_private() => 0,
            IpAddr::V4(_) => 1,
            IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80 => continue,
            IpAddr::V6(_) => 2,
        };
        let sock = SocketAddr::new(ip, port);
        if !ranked.iter().any(|(_, existing)| *existing == sock) {
            ranked.push((prio, sock));
        }
    }

    #[cfg(not(target_os = "ios"))]
    {
        let mut enumerated_any = false;
        if let Ok(ifaces) = getifs::interfaces() {
            for iface in ifaces {
                let name = iface.name().as_str();
                let Ok(addrs) = iface.addrs() else {
                    continue;
                };
                for ifnet in addrs {
                    let ip = ifnet.addr();
                    let Some(prio) = classify_interface_priority(name, ip) else {
                        continue;
                    };
                    enumerated_any = true;
                    let sock = SocketAddr::new(ip, port);
                    if !ranked.iter().any(|(_, existing)| *existing == sock) {
                        ranked.push((prio, sock));
                    }
                }
            }
        }

        // Android API 30+ fallback: if RTM_GETLINK interface enumeration failed or returned empty,
        // query routed local addresses directly via `getifs::local_addrs()`.
        if !enumerated_any {
            if let Ok(addrs) = getifs::local_addrs() {
                for ifnet in addrs {
                    let ip = ifnet.addr();
                    let Some(prio) = classify_interface_priority("wlan0", ip) else {
                        continue;
                    };
                    let sock = SocketAddr::new(ip, port);
                    if !ranked.iter().any(|(_, existing)| *existing == sock) {
                        ranked.push((prio, sock));
                    }
                }
            }
        }
    }

    // iOS omits BSD routing-socket `libc::rt_msghdr`/`RTM_*` constants used by `getifs`,
    // whereas Apple's POSIX `getifaddrs(3)` via `if-addrs` works natively on iOS.
    #[cfg(target_os = "ios")]
    {
        if let Ok(ifaces) = if_addrs::get_if_addrs() {
            for iface in ifaces {
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
    }

    ranked.sort_by_key(|(prio, _)| *prio);

    if include_vpn_fallback {
        ranked.into_iter().map(|(_, addr)| addr).collect()
    } else {
        // For LAN mDNS, if real physical LAN NICs exist (priority <= 2), omit VPN fallbacks.
        let has_physical = ranked.iter().any(|(prio, _)| *prio <= 2);
        ranked
            .into_iter()
            .filter(|(prio, _)| !has_physical || *prio <= 2)
            .map(|(_, addr)| addr)
            .collect()
    }
}

fn is_cgnat_or_tailscale_v4(v4: std::net::Ipv4Addr) -> bool {
    let o = v4.octets();
    o[0] == 100 && (o[1] & 0b1100_0000) == 64
}

/// Enumerates local physical LAN addresses bound to `port` (excluding VPN adapters when physical NICs exist).
pub fn local_lan_addrs_with_overrides(port: u16, overrides: &[IpAddr]) -> Vec<SocketAddr> {
    local_addrs_with_options(port, overrides, false)
}

/// Enumerates local LAN, global IPv6, and mesh VPN (Tailscale/ZeroTier/WireGuard) addresses for global tickets.
pub fn local_ticket_addrs_with_overrides(port: u16, overrides: &[IpAddr]) -> Vec<SocketAddr> {
    local_addrs_with_options(port, overrides, true)
}

/// Enumerates local network addresses bound to `port` without explicit overrides.
pub fn local_lan_addrs(port: u16) -> Vec<SocketAddr> {
    local_lan_addrs_with_overrides(port, &[])
}

/// Manages mDNS advertisement, sub-second UDP LAN presence beacons, and dynamic Wi-Fi/LAN
/// interface change tracking for nearby Veloce peers.
pub struct LanDiscovery {
    local_node_id: NodeId,
    listen_port: u16,
    daemon: ServiceDaemon,
    service_fullname: Arc<RwLock<String>>,
    override_addrs: Arc<RwLock<Vec<IpAddr>>>,
    peers: Arc<RwLock<HashMap<NodeId, PeerEntry>>>,
    beacon_sock: Option<Arc<std::net::UdpSocket>>,
    events_tx: broadcast::Sender<DiscoveryEvent>,
    cancel: CancellationToken,
}

impl LanDiscovery {
    /// Starts advertising this node over mDNS + UDP LAN beacons and browsing for nearby peers.
    pub fn start(
        local_node_id: NodeId,
        device_name: String,
        listen_port: u16,
    ) -> Result<Self> {
        Self::start_with_overrides(local_node_id, device_name, listen_port, &[])
    }

    /// Starts mDNS + UDP LAN presence advertisement/browsing using explicit platform LAN IP overrides if provided.
    pub fn start_with_overrides(
        local_node_id: NodeId,
        device_name: String,
        listen_port: u16,
        override_lan_addrs: &[IpAddr],
    ) -> Result<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| TransportError::Discovery(format!("failed to start mDNS daemon: {e}")))?;

        let instance_name = format!("veloce-{}", local_node_id.short_id());
        let host_name = format!("{instance_name}.local.");
        let initial_ips: Vec<IpAddr> =
            local_lan_addrs_with_overrides(listen_port, override_lan_addrs)
                .into_iter()
                .map(|a| a.ip())
                .collect();

        let service_fullname_str = if !initial_ips.is_empty() {
            match build_mdns_service_info(
                &instance_name,
                &host_name,
                &initial_ips,
                listen_port,
                local_node_id,
                &device_name,
            ) {
                Ok(info) => {
                    let fullname = info.get_fullname().to_string();
                    let _ = daemon.register(info);
                    fullname
                }
                Err(_) => String::new(),
            }
        } else {
            String::new()
        };

        let receiver = daemon
            .browse(MDNS_SERVICE_TYPE)
            .map_err(|e| TransportError::Discovery(format!("mDNS browse failed: {e}")))?;

        let service_fullname = Arc::new(RwLock::new(service_fullname_str));
        let override_addrs = Arc::new(RwLock::new(override_lan_addrs.to_vec()));
        let peers: Arc<RwLock<HashMap<NodeId, PeerEntry>>> = Arc::new(RwLock::new(HashMap::new()));
        let (events_tx, _) = broadcast::channel(128);
        let cancel = CancellationToken::new();

        // Bind optional UDP LAN presence socket (`:53539`, falling back to `:0` for sending if busy).
        let (beacon_recv_sock, beacon_send_sock) = bind_lan_beacon_sockets();

        let peers_mdns = Arc::clone(&peers);
        let tx_mdns = events_tx.clone();
        let cancel_mdns = cancel.clone();

        // 1. Background mDNS reader thread.
        std::thread::Builder::new()
            .name("veloce-mdns-browser".into())
            .spawn(move || {
                let mut fullname_to_node: HashMap<String, NodeId> = HashMap::new();
                while !cancel_mdns.is_cancelled() {
                    match receiver.recv_timeout(Duration::from_millis(250)) {
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
                            upsert_peer_and_emit(&peers_mdns, &tx_mdns, peer);
                        }
                        Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                            if let Some(removed_id) = fullname_to_node.remove(&fullname) {
                                remove_peer_and_emit(&peers_mdns, &tx_mdns, &removed_id);
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

        // 2. Background UDP LAN beacon receiver thread for sub-second peer join/goodbye updates.
        if let Some(recv_sock) = beacon_recv_sock {
            let peers_b = Arc::clone(&peers);
            let tx_b = events_tx.clone();
            let cancel_b = cancel.clone();
            let _ = std::thread::Builder::new()
                .name("veloce-lan-beacon-rx".into())
                .spawn(move || {
                    let mut buf = [0u8; 1500];
                    while !cancel_b.is_cancelled() {
                        let Ok((len, src_addr)) = recv_sock.recv_from(&mut buf) else {
                            continue;
                        };
                        if len <= LAN_BEACON_MAGIC.len() || buf[..4] != LAN_BEACON_MAGIC {
                            continue;
                        }
                        let Ok(pkt) = postcard::from_bytes::<LanBeaconPacket>(&buf[4..len]) else {
                            continue;
                        };
                        match pkt {
                            LanBeaconPacket::Hello {
                                node_id,
                                device_name,
                                listen_port: port,
                                protocol_version,
                                ips,
                            } => {
                                if node_id == local_node_id || port == 0 {
                                    continue;
                                }
                                let mut addresses: Vec<SocketAddr> = Vec::with_capacity(ips.len() + 1);
                                let observed_sock = SocketAddr::new(src_addr.ip(), port);
                                if !src_addr.ip().is_unspecified() && !src_addr.ip().is_loopback() {
                                    addresses.push(observed_sock);
                                }
                                for ip in ips {
                                    let sock = SocketAddr::new(ip, port);
                                    if !addresses.contains(&sock) {
                                        addresses.push(sock);
                                    }
                                }
                                addresses.sort_by_key(|a| if a.is_ipv4() { 0u8 } else { 1u8 });
                                if addresses.is_empty() {
                                    continue;
                                }
                                let peer = LocalPeer {
                                    node_id,
                                    device_name,
                                    addresses,
                                    port,
                                    protocol_version,
                                };
                                upsert_peer_and_emit(&peers_b, &tx_b, peer);
                            }
                            LanBeaconPacket::Goodbye { node_id } => {
                                if node_id != local_node_id {
                                    remove_peer_and_emit(&peers_b, &tx_b, &node_id);
                                }
                            }
                        }
                    }
                });
        }

        // 3. Background 750ms local interface watcher + UDP beacon broadcaster:
        //    - Detects Wi-Fi <-> Mobile Data transitions in < 0.75s, immediately clearing stale LAN
        //      peers when leaving Wi-Fi and re-announcing mDNS + UDP bursts when joining Wi-Fi!
        {
            let daemon_w = daemon.clone();
            let fullname_w = Arc::clone(&service_fullname);
            let overrides_w = Arc::clone(&override_addrs);
            let peers_w = Arc::clone(&peers);
            let tx_w = events_tx.clone();
            let cancel_w = cancel.clone();
            let send_sock_w = beacon_send_sock.clone();
            let dev_name_w = device_name;

            let _ = std::thread::Builder::new()
                .name("veloce-lan-watcher".into())
                .spawn(move || {
                    let mut last_ips = initial_ips;
                    if !last_ips.is_empty() {
                        if let Some(ref sock) = send_sock_w {
                            broadcast_lan_hello(
                                sock,
                                local_node_id,
                                &dev_name_w,
                                listen_port,
                                &last_ips,
                            );
                        }
                    }

                    while !cancel_w.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(750));
                        if cancel_w.is_cancelled() {
                            break;
                        }

                        let ov = overrides_w
                            .read()
                            .expect("overrides lock poisoned")
                            .clone();
                        let current_ips: Vec<IpAddr> =
                            local_lan_addrs_with_overrides(listen_port, &ov)
                                .into_iter()
                                .map(|a| a.ip())
                                .collect();

                        if current_ips != last_ips {
                            // Unregister previous mDNS service record if one existed.
                            {
                                let mut fname =
                                    fullname_w.write().expect("fullname lock poisoned");
                                if !fname.is_empty() {
                                    let _ = daemon_w.unregister(&fname);
                                    fname.clear();
                                }
                            }

                            if current_ips.is_empty() {
                                // Left Wi-Fi / switched to Mobile Data: evict all LAN peers immediately!
                                clear_all_peers_and_emit(&peers_w, &tx_w);
                            } else {
                                // Joined Wi-Fi or changed LAN IP: re-register mDNS & fire immediate Hello burst!
                                if let Ok(info) = build_mdns_service_info(
                                    &instance_name,
                                    &host_name,
                                    &current_ips,
                                    listen_port,
                                    local_node_id,
                                    &dev_name_w,
                                ) {
                                    let new_fname = info.get_fullname().to_string();
                                    if daemon_w.register(info).is_ok() {
                                        *fullname_w.write().expect("fullname lock poisoned") =
                                            new_fname;
                                    }
                                }
                                if let Some(ref sock) = send_sock_w {
                                    broadcast_lan_hello(
                                        sock,
                                        local_node_id,
                                        &dev_name_w,
                                        listen_port,
                                        &current_ips,
                                    );
                                }
                            }
                            last_ips = current_ips;
                        } else if !last_ips.is_empty() {
                            if let Some(ref sock) = send_sock_w {
                                broadcast_lan_hello(
                                    sock,
                                    local_node_id,
                                    &dev_name_w,
                                    listen_port,
                                    &last_ips,
                                );
                            }
                        }
                    }
                });
        }

        Ok(Self {
            local_node_id,
            listen_port,
            daemon,
            service_fullname,
            override_addrs,
            peers,
            beacon_sock: beacon_send_sock,
            events_tx,
            cancel,
        })
    }

    /// Updates platform-supplied LAN IP overrides (e.g. Android `ConnectivityManager` callbacks).
    pub fn update_overrides(&self, addrs: Vec<IpAddr>) {
        *self
            .override_addrs
            .write()
            .expect("overrides lock poisoned") = addrs.clone();
        let current_ips: Vec<IpAddr> = local_lan_addrs_with_overrides(self.listen_port, &addrs)
            .into_iter()
            .map(|a| a.ip())
            .collect();
        if current_ips.is_empty() {
            clear_all_peers_and_emit(&self.peers, &self.events_tx);
        }
    }

    /// Refreshes `last_seen` when an active unicast probe receives a `HolePunchAck`.
    pub fn touch_peer(&self, node_id: &NodeId) {
        if let Some(entry) = self
            .peers
            .write()
            .expect("peers lock poisoned")
            .get_mut(node_id)
        {
            entry.last_seen = Instant::now();
        }
    }

    /// Returns elapsed time since `node_id` was last heard via mDNS, UDP beacon, or unicast ACK.
    pub fn last_seen_elapsed(&self, node_id: &NodeId) -> Option<Duration> {
        self.peers
            .read()
            .expect("peers lock poisoned")
            .get(node_id)
            .map(|e| e.last_seen.elapsed())
    }

    /// Immediately removes an unreachable peer and emits `DiscoveryEvent::PeerLost`.
    pub fn evict_peer(&self, node_id: &NodeId) -> bool {
        remove_peer_and_emit(&self.peers, &self.events_tx, node_id)
    }

    pub fn list_peers(&self) -> Vec<LocalPeer> {
        let map = self.peers.read().expect("peers lock poisoned");
        map.values().map(|e| e.peer.clone()).collect()
    }

    pub fn get_peer(&self, node_id: &NodeId) -> Option<LocalPeer> {
        let map = self.peers.read().expect("peers lock poisoned");
        map.get(node_id).map(|e| e.peer.clone())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DiscoveryEvent> {
        self.events_tx.subscribe()
    }

    pub fn shutdown(&self) {
        if !self.cancel.is_cancelled() {
            self.cancel.cancel();
            if let Some(ref sock) = self.beacon_sock {
                let ov = self
                    .override_addrs
                    .read()
                    .expect("overrides lock poisoned")
                    .clone();
                let ips: Vec<IpAddr> = local_lan_addrs_with_overrides(self.listen_port, &ov)
                    .into_iter()
                    .map(|a| a.ip())
                    .collect();
                broadcast_lan_goodbye(sock, self.local_node_id, &ips);
            }
            let fname = self
                .service_fullname
                .read()
                .expect("fullname lock poisoned")
                .clone();
            if !fname.is_empty() {
                let _ = self.daemon.unregister(&fname);
            }
            let _ = self.daemon.shutdown();
        }
    }
}

impl Drop for LanDiscovery {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn build_mdns_service_info(
    instance_name: &str,
    host_name: &str,
    ips: &[IpAddr],
    listen_port: u16,
    local_node_id: NodeId,
    device_name: &str,
) -> Result<ServiceInfo> {
    let mut properties = HashMap::new();
    properties.insert("node_id".to_string(), local_node_id.to_base32());
    properties.insert("name".to_string(), device_name.to_string());
    properties.insert("ver".to_string(), PROTOCOL_VERSION.to_string());

    ServiceInfo::new(
        MDNS_SERVICE_TYPE,
        instance_name,
        host_name,
        ips,
        listen_port,
        Some(properties),
    )
    .map_err(|e| TransportError::Discovery(format!("invalid mDNS ServiceInfo: {e}")))
}

fn upsert_peer_and_emit(
    peers: &Arc<RwLock<HashMap<NodeId, PeerEntry>>>,
    events_tx: &broadcast::Sender<DiscoveryEvent>,
    peer: LocalPeer,
) {
    let should_emit = {
        let mut map = peers.write().expect("peers lock poisoned");
        let changed = match map.get(&peer.node_id) {
            Some(existing) => existing.peer != peer,
            None => true,
        };
        map.insert(
            peer.node_id,
            PeerEntry {
                peer: peer.clone(),
                last_seen: Instant::now(),
            },
        );
        changed
    };
    if should_emit {
        let _ = events_tx.send(DiscoveryEvent::PeerDiscovered(peer));
    }
}

fn remove_peer_and_emit(
    peers: &Arc<RwLock<HashMap<NodeId, PeerEntry>>>,
    events_tx: &broadcast::Sender<DiscoveryEvent>,
    node_id: &NodeId,
) -> bool {
    let removed = peers
        .write()
        .expect("peers lock poisoned")
        .remove(node_id)
        .is_some();
    if removed {
        let _ = events_tx.send(DiscoveryEvent::PeerLost(*node_id));
    }
    removed
}

fn clear_all_peers_and_emit(
    peers: &Arc<RwLock<HashMap<NodeId, PeerEntry>>>,
    events_tx: &broadcast::Sender<DiscoveryEvent>,
) {
    let removed_ids: Vec<NodeId> = {
        let mut map = peers.write().expect("peers lock poisoned");
        let ids = map.keys().copied().collect();
        map.clear();
        ids
    };
    for id in removed_ids {
        let _ = events_tx.send(DiscoveryEvent::PeerLost(id));
    }
}

fn bind_lan_beacon_sockets() -> (
    Option<std::net::UdpSocket>,
    Option<Arc<std::net::UdpSocket>>,
) {
    if let Ok(sock) = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)) {
        let _ = sock.set_reuse_address(true);
        let _ = sock.set_broadcast(true);
        let _ = sock.set_multicast_loop_v4(true);
        let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, LAN_BEACON_PORT);
        if sock.bind(&bind_addr.into()).is_ok() {
            let _ = sock.join_multicast_v4(&LAN_BEACON_MULTICAST, &Ipv4Addr::UNSPECIFIED);
            let _ = sock.set_read_timeout(Some(Duration::from_millis(250)));
            let recv_std: std::net::UdpSocket = sock.into();
            let send_std = recv_std.try_clone().ok().map(Arc::new);
            return (Some(recv_std), send_std);
        }
    }

    // Fallback ephemeral sender socket if `:53539` is already bound by another local instance.
    let send_only = std::net::UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        .ok()
        .map(|s| {
            let _ = s.set_broadcast(true);
            Arc::new(s)
        });
    (None, send_only)
}

fn beacon_targets_for_ips(ips: &[IpAddr]) -> Vec<SocketAddr> {
    let mut targets = vec![
        SocketAddr::V4(SocketAddrV4::new(LAN_BEACON_MULTICAST, LAN_BEACON_PORT)),
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, LAN_BEACON_PORT)),
    ];
    for ip in ips {
        if let IpAddr::V4(v4) = ip {
            if v4.is_private() {
                let o = v4.octets();
                let subnet_bcast =
                    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(o[0], o[1], o[2], 255), LAN_BEACON_PORT));
                if !targets.contains(&subnet_bcast) {
                    targets.push(subnet_bcast);
                }
            }
        }
    }
    targets
}

fn broadcast_lan_hello(
    sock: &std::net::UdpSocket,
    node_id: NodeId,
    device_name: &str,
    listen_port: u16,
    ips: &[IpAddr],
) {
    let pkt = LanBeaconPacket::Hello {
        node_id,
        device_name: device_name.to_string(),
        listen_port,
        protocol_version: PROTOCOL_VERSION,
        ips: ips.iter().copied().take(8).collect(),
    };
    let Ok(encoded) = postcard::to_allocvec(&pkt) else {
        return;
    };
    let mut wire = Vec::with_capacity(LAN_BEACON_MAGIC.len() + encoded.len());
    wire.extend_from_slice(&LAN_BEACON_MAGIC);
    wire.extend_from_slice(&encoded);
    for target in beacon_targets_for_ips(ips) {
        let _ = sock.send_to(&wire, target);
    }
}

fn broadcast_lan_goodbye(sock: &std::net::UdpSocket, node_id: NodeId, ips: &[IpAddr]) {
    let pkt = LanBeaconPacket::Goodbye { node_id };
    let Ok(encoded) = postcard::to_allocvec(&pkt) else {
        return;
    };
    let mut wire = Vec::with_capacity(LAN_BEACON_MAGIC.len() + encoded.len());
    wire.extend_from_slice(&LAN_BEACON_MAGIC);
    wire.extend_from_slice(&encoded);
    for target in beacon_targets_for_ips(ips) {
        let _ = sock.send_to(&wire, target);
    }
}
