use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use core_protocol::{ConnectionType, NodeId, SignalingMessage};
use rand::RngCore;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::discovery::local_lan_addrs;
use crate::endpoint::classify_socket_addr;
use crate::error::{Result, TransportError};
use crate::magic_socket::MagicUdpSocket;

/// Result of resolving and traversing NAT toward a remote peer.
#[derive(Debug, Clone)]
pub struct NatConnectPlan {
    /// Socket address to dial with QUIC (either direct or `MagicUdpSocket` virtual address).
    pub dial_addr: SocketAddr,
    /// Initial active connection classification (`Lan`, `DirectP2p`, or `Relayed`).
    pub connection_type: ConnectionType,
    /// Candidate direct addresses for continuous background upgrade probing when `Relayed`.
    pub upgrade_candidates: Vec<SocketAddr>,
}

type LookupResult = (Option<SocketAddr>, Vec<SocketAddr>);

/// Coordinates STUN reflexive address discovery, stateless relay registration,
/// simultaneous UDP hole-punching, and continuous background path upgrades (`Relayed -> DirectP2p`).
pub struct NatCoordinator {
    socket: Arc<MagicUdpSocket>,
    local_node_id: NodeId,
    relay_addrs: RwLock<Vec<SocketAddr>>,
    public_addrs: RwLock<Vec<SocketAddr>>,
    pending_stun: Mutex<HashMap<u64, oneshot::Sender<SocketAddr>>>,
    pending_lookups: Mutex<HashMap<NodeId, oneshot::Sender<LookupResult>>>,
    pending_punches: Mutex<HashMap<NodeId, Vec<oneshot::Sender<SocketAddr>>>>,
    cancel: CancellationToken,
}

impl NatCoordinator {
    /// Starts the NAT coordinator event loop processing incoming signaling packets from `MagicUdpSocket`.
    pub fn start(
        socket: Arc<MagicUdpSocket>,
        mut signaling_rx: mpsc::UnboundedReceiver<(SocketAddr, SignalingMessage)>,
        initial_relays: Vec<SocketAddr>,
        cancel: CancellationToken,
    ) -> Arc<Self> {
        let local_node_id = socket.local_node_id();
        let coord = Arc::new(Self {
            socket,
            local_node_id,
            relay_addrs: RwLock::new(initial_relays),
            public_addrs: RwLock::new(Vec::new()),
            pending_stun: Mutex::new(HashMap::new()),
            pending_lookups: Mutex::new(HashMap::new()),
            pending_punches: Mutex::new(HashMap::new()),
            cancel: cancel.clone(),
        });

        // Signaling dispatch loop.
        let coord_rx = Arc::clone(&coord);
        let cancel_rx = cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_rx.cancelled() => break,
                    item = signaling_rx.recv() => {
                        let Some((from_addr, msg)) = item else { break };
                        coord_rx.handle_signaling(from_addr, msg).await;
                    }
                }
            }
        });

        // Periodic relay heartbeat registration (keeps NAT pinhole and relay table alive).
        let coord_hb = Arc::clone(&coord);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = interval.tick() => {
                        let relays = coord_hb.relay_addrs();
                        for relay in relays {
                            let _ = coord_hb.register_with_relay(relay).await;
                        }
                    }
                }
            }
        });

        coord
    }

    pub fn add_relay(&self, relay_addr: SocketAddr) {
        let mut relays = self.relay_addrs.write().expect("relays lock poisoned");
        if !relays.contains(&relay_addr) {
            relays.push(relay_addr);
        }
    }

    pub fn relay_addrs(&self) -> Vec<SocketAddr> {
        self.relay_addrs
            .read()
            .expect("relays lock poisoned")
            .clone()
    }

    pub fn public_addr(&self) -> Option<SocketAddr> {
        self.public_addrs
            .read()
            .expect("public_addrs lock poisoned")
            .first()
            .copied()
    }

    pub fn public_addrs(&self) -> Vec<SocketAddr> {
        self.public_addrs
            .read()
            .expect("public_addrs lock poisoned")
            .clone()
    }

    fn record_public_addr(&self, observed: SocketAddr) {
        if observed.ip().is_unspecified() || observed.ip().is_loopback() {
            return;
        }
        let mut addrs = self.public_addrs.write().expect("public_addrs lock poisoned");
        if !addrs.contains(&observed) {
            // Keep IPv4 first when present for broadest peer compatibility.
            if observed.is_ipv4() {
                addrs.insert(0, observed);
            } else {
                addrs.push(observed);
            }
        }
    }

    /// Discovers our reflexive public UDP address using STUN against the specified relay.
    pub async fn discover_public_addr(&self, relay_addr: SocketAddr) -> Result<SocketAddr> {
        let tx_id = rand::thread_rng().next_u64();
        let (tx, rx) = oneshot::channel();
        self.pending_stun
            .lock()
            .expect("pending_stun lock poisoned")
            .insert(tx_id, tx);

        self.socket
            .send_signaling(relay_addr, &SignalingMessage::StunRequest { tx_id })
            .await?;

        let observed = tokio::time::timeout(Duration::from_secs(3), rx)
            .await
            .map_err(|_| TransportError::Timeout("STUN discovery timed out".into()))?
            .map_err(|_| TransportError::NatTraversal("STUN waiter dropped".into()))?;

        self.record_public_addr(observed);
        Ok(observed)
    }

    /// Queries standard RFC 5389 public STUN servers (`stun.cloudflare.com:3478`,
    /// `stun.l.google.com:19302`) on the shared QUIC UDP socket across both IPv4 and IPv6
    /// (when dual-stack) so global tickets include reflexive public endpoints at $0 cost.
    pub async fn discover_public_addrs_rfc5389(&self) -> Vec<SocketAddr> {
        let mut discovered = Vec::new();
        let v4_fut = async {
            if self.socket.supports_ipv4() {
                self.query_rfc5389_family(false).await
            } else {
                None
            }
        };
        let v6_fut = async {
            if self.socket.supports_ipv6() {
                self.query_rfc5389_family(true).await
            } else {
                None
            }
        };

        let (v4_obs, v6_obs) = tokio::join!(v4_fut, v6_fut);
        if let Some(a) = v4_obs {
            self.record_public_addr(a);
            discovered.push(a);
        }
        if let Some(a) = v6_obs {
            self.record_public_addr(a);
            if !discovered.contains(&a) {
                discovered.push(a);
            }
        }
        discovered
    }

    /// Convenience wrapper returning the first RFC 5389 discovered public address.
    pub async fn discover_public_addr_rfc5389(&self) -> Option<SocketAddr> {
        self.discover_public_addrs_rfc5389().await.into_iter().next()
    }

    async fn query_rfc5389_family(&self, want_ipv6: bool) -> Option<SocketAddr> {
        const PUBLIC_STUN_HOSTS: &[&str] = &[
            "stun.cloudflare.com:3478",
            "stun.l.google.com:19302",
        ];

        for host in PUBLIC_STUN_HOSTS {
            let Ok(Ok(mut resolved)) =
                tokio::time::timeout(Duration::from_millis(600), tokio::net::lookup_host(*host))
                    .await
            else {
                continue;
            };
            let Some(stun_addr) = resolved.find(|a| a.is_ipv6() == want_ipv6) else {
                continue;
            };

            let tx_id = rand::thread_rng().next_u64();
            let (tx, rx) = oneshot::channel();
            self.pending_stun
                .lock()
                .expect("pending_stun lock poisoned")
                .insert(tx_id, tx);

            // 20-byte RFC 5389 Binding Request:
            // [0x00, 0x01] Binding Request | [0x00, 0x00] Length 0
            // [0x21, 0x12, 0xA4, 0x42] Magic Cookie | 12-byte Transaction ID (4 random + 8 tx_id)
            let mut pkt = [0u8; 20];
            pkt[0] = 0x00;
            pkt[1] = 0x01;
            pkt[4..8].copy_from_slice(&0x2112_A442u32.to_be_bytes());
            rand::thread_rng().fill_bytes(&mut pkt[8..12]);
            pkt[12..20].copy_from_slice(&tx_id.to_be_bytes());

            if self.socket.send_raw_datagram(stun_addr, &pkt).await.is_ok() {
                if let Ok(Ok(observed)) = tokio::time::timeout(Duration::from_millis(750), rx).await
                {
                    return Some(observed);
                }
            }
            self.pending_stun
                .lock()
                .expect("pending_stun lock poisoned")
                .remove(&tx_id);
        }

        None
    }

    /// Registers this node's Ed25519 `NodeId` and LAN addresses with a stateless relay server.
    pub async fn register_with_relay(&self, relay_addr: SocketAddr) -> Result<()> {
        let port = self.socket.local_addr()?.port();
        let local_addrs = local_lan_addrs(port);
        self.socket
            .send_signaling(
                relay_addr,
                &SignalingMessage::RelayRegister {
                    node_id: self.local_node_id,
                    local_addrs,
                },
            )
            .await?;
        Ok(())
    }

    /// Looks up a remote peer via relay (simultaneously sending `PunchNotify` to the peer)
    /// and attempts UDP hole-punching, falling back seamlessly to end-to-end encrypted relay.
    pub async fn resolve_peer_via_relay(
        &self,
        target_node_id: NodeId,
        relay_addr: SocketAddr,
        extra_direct_hints: &[SocketAddr],
        punch_timeout: Duration,
    ) -> Result<NatConnectPlan> {
        self.add_relay(relay_addr);
        self.register_with_relay(relay_addr).await?;

        let (tx, rx) = oneshot::channel();
        self.pending_lookups
            .lock()
            .expect("pending_lookups lock poisoned")
            .insert(target_node_id, tx);

        let port = self.socket.local_addr()?.port();
        self.socket
            .send_signaling(
                relay_addr,
                &SignalingMessage::PeerLookup {
                    target_node_id,
                    requester_node_id: self.local_node_id,
                    requester_local_addrs: local_lan_addrs(port),
                },
            )
            .await?;

        let (observed_addr, peer_local_addrs) =
            tokio::time::timeout(Duration::from_secs(3), rx)
                .await
                .map_err(|_| TransportError::Timeout("relay peer lookup timed out".into()))?
                .map_err(|_| TransportError::NatTraversal("lookup channel closed".into()))?;

        let mut candidates: Vec<SocketAddr> = Vec::new();
        if let Some(obs) = observed_addr {
            candidates.push(obs);
        }
        for addr in peer_local_addrs.into_iter().chain(extra_direct_hints.iter().copied()) {
            if !candidates.contains(&addr) {
                candidates.push(addr);
            }
        }

        // If relay-only mode is forced (e.g. symmetric NAT simulation), skip direct punch and use relay immediately.
        if !self.socket.is_force_relay() && !candidates.is_empty() {
            if let Some(punched_addr) = self
                .attempt_hole_punch(target_node_id, &candidates, punch_timeout)
                .await
            {
                let conn_type = classify_socket_addr(punched_addr);
                let effective_type = match conn_type {
                    ConnectionType::Lan => ConnectionType::DirectP2p,
                    other => other,
                };
                let virt = self.socket.ensure_virtual_route(
                    target_node_id,
                    Some(relay_addr),
                    Some(punched_addr),
                    effective_type,
                );
                return Ok(NatConnectPlan {
                    dial_addr: virt,
                    connection_type: effective_type,
                    upgrade_candidates: candidates,
                });
            }
        }

        // Fallback to end-to-end encrypted relay route; QUIC runs immediately over `virt`.
        let virt = self.socket.ensure_virtual_route(
            target_node_id,
            Some(relay_addr),
            None,
            ConnectionType::Relayed,
        );
        Ok(NatConnectPlan {
            dial_addr: virt,
            connection_type: ConnectionType::Relayed,
            upgrade_candidates: candidates,
        })
    }

    /// Sends simultaneous UDP hole-punch probes to all candidate endpoints and awaits a `HolePunchAck`.
    pub async fn attempt_hole_punch(
        &self,
        target_node_id: NodeId,
        candidates: &[SocketAddr],
        timeout: Duration,
    ) -> Option<SocketAddr> {
        if candidates.is_empty() || self.socket.is_force_relay() {
            return None;
        }
        let (tx, rx) = oneshot::channel();
        self.pending_punches
            .lock()
            .expect("pending_punches lock poisoned")
            .entry(target_node_id)
            .or_default()
            .push(tx);

        // Send 3 staggered bursts across all candidate endpoints to punch stateful NAT firewalls.
        let sock = Arc::clone(&self.socket);
        let local_id = self.local_node_id;
        let targets = candidates.to_vec();
        let probe_task = tokio::spawn(async move {
            for attempt in 0..3u64 {
                let probe = SignalingMessage::HolePunchProbe {
                    tx_id: attempt,
                    sender_node_id: local_id,
                };
                for &addr in &targets {
                    let _ = sock.send_signaling(addr, &probe).await;
                }
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
        });

        let result = tokio::time::timeout(timeout, rx).await.ok().and_then(|r| r.ok());
        probe_task.abort();
        result
    }

    /// Spawns a background task continuously attempting to upgrade a `Relayed` peer to `DirectP2p`.
    pub fn spawn_background_upgrade(
        self: &Arc<Self>,
        target_node_id: NodeId,
        candidates: Vec<SocketAddr>,
        session_cancel: CancellationToken,
    ) {
        if candidates.is_empty() {
            return;
        }
        let coord = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(400));
            loop {
                tokio::select! {
                    _ = coord.cancel.cancelled() => break,
                    _ = session_cancel.cancelled() => break,
                    _ = interval.tick() => {
                        if coord.socket.is_force_relay() {
                            continue;
                        }
                        if matches!(
                            coord.socket.connection_type_for_peer(&target_node_id),
                            Some(ConnectionType::DirectP2p) | Some(ConnectionType::Lan)
                        ) {
                            break;
                        }
                        if let Some(direct_addr) = coord
                            .attempt_hole_punch(target_node_id, &candidates, Duration::from_millis(350))
                            .await
                        {
                            let upgraded = coord.socket.upgrade_peer_to_direct(
                                target_node_id,
                                direct_addr,
                                ConnectionType::DirectP2p,
                            );
                            if upgraded {
                                tracing::info!(
                                    peer = %target_node_id.short_id(),
                                    %direct_addr,
                                    "Upgraded active relayed session to Direct-P2P"
                                );
                                break;
                            }
                        }
                    }
                }
            }
        });
    }

    async fn handle_signaling(&self, from_addr: SocketAddr, msg: SignalingMessage) {
        match msg {
            SignalingMessage::StunResponse {
                tx_id,
                observed_addr,
            } => {
                self.record_public_addr(observed_addr);
                if let Some(waiter) = self
                    .pending_stun
                    .lock()
                    .expect("pending_stun lock poisoned")
                    .remove(&tx_id)
                {
                    let _ = waiter.send(observed_addr);
                }
            }
            SignalingMessage::RelayRegistered { observed_addr, .. } => {
                self.record_public_addr(observed_addr);
            }
            SignalingMessage::PeerEndpoints {
                target_node_id,
                observed_addr,
                local_addrs,
            } => {
                if let Some(waiter) = self
                    .pending_lookups
                    .lock()
                    .expect("pending_lookups lock poisoned")
                    .remove(&target_node_id)
                {
                    let _ = waiter.send((observed_addr, local_addrs));
                }
            }
            SignalingMessage::PunchNotify {
                from_node_id,
                from_observed_addr,
                from_local_addrs,
            } => {
                if self.socket.is_force_relay() {
                    return;
                }
                // Immediately fire simultaneous hole-punch probes back toward the dialing peer!
                let probe = SignalingMessage::HolePunchProbe {
                    tx_id: rand::thread_rng().next_u64(),
                    sender_node_id: self.local_node_id,
                };
                let _ = self.socket.send_signaling(from_observed_addr, &probe).await;
                for addr in from_local_addrs {
                    if addr != from_observed_addr {
                        let _ = self.socket.send_signaling(addr, &probe).await;
                    }
                }
                tracing::debug!(
                    from = %from_node_id.short_id(),
                    %from_observed_addr,
                    "Responded to PunchNotify with simultaneous UDP hole-punch probes"
                );
            }
            SignalingMessage::HolePunchProbe {
                tx_id,
                sender_node_id,
            } => {
                if self.socket.is_force_relay() {
                    return;
                }
                // Acknowledge the direct UDP probe and upgrade our route if this peer already has a relayed session.
                let ack = SignalingMessage::HolePunchAck {
                    tx_id,
                    sender_node_id: self.local_node_id,
                };
                let _ = self.socket.send_signaling(from_addr, &ack).await;
                self.socket.upgrade_peer_to_direct(
                    sender_node_id,
                    from_addr,
                    ConnectionType::DirectP2p,
                );
            }
            SignalingMessage::HolePunchAck { sender_node_id, .. } => {
                if self.socket.is_force_relay() {
                    return;
                }
                self.socket.upgrade_peer_to_direct(
                    sender_node_id,
                    from_addr,
                    ConnectionType::DirectP2p,
                );
                if let Some(waiters) = self
                    .pending_punches
                    .lock()
                    .expect("pending_punches lock poisoned")
                    .remove(&sender_node_id)
                {
                    for w in waiters {
                        let _ = w.send(from_addr);
                    }
                }
            }
            _ => {}
        }
    }
}
