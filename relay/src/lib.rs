use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_protocol::{NodeId, SignalingMessage};
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

/// Default peer registration lifetime in the stateless in-memory routing table.
pub const REGISTRATION_TTL: Duration = Duration::from_secs(120);

/// Strict maximum UDP datagram accepted by `veloce-relay`.
/// Covers 1380-byte QUIC packets + ~90-byte `RelayForward` envelope with comfortable headroom,
/// while preventing memory abuse and IP fragmentation attacks.
pub const DEFAULT_MAX_DATAGRAM_BYTES: usize = 2048;

/// Maximum opaque QUIC payload allowed inside `SignalingMessage::RelayForward`.
pub const DEFAULT_MAX_RELAY_PAYLOAD_BYTES: usize = 1600;

/// Security & resource limits governing `RelayServer`.
#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub registration_ttl: Duration,
    pub max_datagram_bytes: usize,
    pub max_relay_payload_bytes: usize,
    /// Sustained packets/sec allowed per source IP (covering relayed QUIC data streams).
    pub max_packets_per_sec_per_ip: f64,
    pub packet_burst_per_ip: f64,
    /// Sustained bytes/sec allowed per source IP.
    pub max_bytes_per_sec_per_ip: f64,
    pub byte_burst_per_ip: f64,
    /// Tighter anti-amplification rate limit for unauthenticated control/STUN/lookup packets per IP.
    pub max_control_per_sec_per_ip: f64,
    pub control_burst_per_ip: f64,
    /// Global cap on active peer registrations in memory.
    pub max_total_peers: usize,
    /// Cap on active peer registrations originating from a single IP address.
    pub max_peers_per_ip: usize,
    /// Maximum `local_addrs` hints stored per peer (caps `PeerEndpoints`/`PunchNotify` size).
    pub max_local_addrs_per_peer: usize,
    /// Maximum source IPs tracked in the rate-limiter table before evicting idle entries.
    pub max_tracked_ips: usize,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            registration_ttl: REGISTRATION_TTL,
            max_datagram_bytes: DEFAULT_MAX_DATAGRAM_BYTES,
            max_relay_payload_bytes: DEFAULT_MAX_RELAY_PAYLOAD_BYTES,
            max_packets_per_sec_per_ip: 4_000.0,
            packet_burst_per_ip: 8_000.0,
            max_bytes_per_sec_per_ip: 8.0 * 1024.0 * 1024.0,
            byte_burst_per_ip: 16.0 * 1024.0 * 1024.0,
            max_control_per_sec_per_ip: 30.0,
            control_burst_per_ip: 60.0,
            max_total_peers: 10_000,
            max_peers_per_ip: 16,
            max_local_addrs_per_peer: 8,
            max_tracked_ips: 65_536,
        }
    }
}

#[derive(Debug, Clone)]
struct PeerEntry {
    observed_addr: SocketAddr,
    local_addrs: Vec<SocketAddr>,
    last_seen: Instant,
}

#[derive(Debug, Clone)]
struct IpTokenBucket {
    pkt_tokens: f64,
    byte_tokens: f64,
    ctrl_tokens: f64,
    last_refill: Instant,
}

impl IpTokenBucket {
    fn new(cfg: &RelayConfig, now: Instant) -> Self {
        Self {
            pkt_tokens: cfg.packet_burst_per_ip,
            byte_tokens: cfg.byte_burst_per_ip,
            ctrl_tokens: cfg.control_burst_per_ip,
            last_refill: now,
        }
    }

    fn refill(&mut self, cfg: &RelayConfig, now: Instant) {
        let dt = now.duration_since(self.last_refill).as_secs_f64();
        if dt > 0.0 {
            self.pkt_tokens =
                (self.pkt_tokens + dt * cfg.max_packets_per_sec_per_ip).min(cfg.packet_burst_per_ip);
            self.byte_tokens =
                (self.byte_tokens + dt * cfg.max_bytes_per_sec_per_ip).min(cfg.byte_burst_per_ip);
            self.ctrl_tokens = (self.ctrl_tokens + dt * cfg.max_control_per_sec_per_ip)
                .min(cfg.control_burst_per_ip);
            self.last_refill = now;
        }
    }

    fn allow_packet(&mut self, cfg: &RelayConfig, bytes: usize, now: Instant) -> bool {
        self.refill(cfg, now);
        let cost_bytes = bytes as f64;
        if self.pkt_tokens >= 1.0 && self.byte_tokens >= cost_bytes {
            self.pkt_tokens -= 1.0;
            self.byte_tokens -= cost_bytes;
            true
        } else {
            false
        }
    }

    fn allow_control(&mut self, cfg: &RelayConfig, now: Instant) -> bool {
        self.refill(cfg, now);
        if self.ctrl_tokens >= 1.0 {
            self.ctrl_tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Lightweight telemetry counters for observability, abuse monitoring, and testing.
#[derive(Debug, Default)]
pub struct RelayStats {
    pub stun_requests: AtomicU64,
    pub registrations: AtomicU64,
    pub lookups: AtomicU64,
    pub forwarded_packets: AtomicU64,
    pub forwarded_bytes: AtomicU64,
    pub rate_limited_packets: AtomicU64,
    pub oversized_packets: AtomicU64,
    pub registration_cap_drops: AtomicU64,
    pub malformed_packets: AtomicU64,
}

/// Minimal stateless UDP STUN, hole-punch coordinator, and blind encrypted relay server
/// hardened with per-IP token-bucket rate limiting, anti-amplification controls,
/// strict max datagram caps, and global/per-IP peer registration limits.
pub struct RelayServer {
    socket: Arc<UdpSocket>,
    local_addr: SocketAddr,
    stats: Arc<RelayStats>,
    cancel: CancellationToken,
}

impl RelayServer {
    /// Binds the relay UDP socket using default production security limits.
    pub async fn bind(bind_addr: SocketAddr) -> std::io::Result<Self> {
        Self::bind_with_config(bind_addr, RelayConfig::default()).await
    }

    /// Binds the relay UDP socket with custom security/rate-limit configuration.
    pub async fn bind_with_config(
        bind_addr: SocketAddr,
        config: RelayConfig,
    ) -> std::io::Result<Self> {
        let socket = Arc::new(UdpSocket::bind(bind_addr).await?);
        let local_addr = socket.local_addr()?;
        let stats = Arc::new(RelayStats::default());
        let cancel = CancellationToken::new();

        let sock_bg = Arc::clone(&socket);
        let stats_bg = Arc::clone(&stats);
        let cancel_bg = cancel.clone();

        tokio::spawn(async move {
            run_loop(sock_bg, config, stats_bg, cancel_bg).await;
        });

        Ok(Self {
            socket,
            local_addr,
            stats,
            cancel,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn stats(&self) -> &Arc<RelayStats> {
        &self.stats
    }

    pub fn shutdown(&self) {
        self.cancel.cancel();
    }

    pub async fn wait_until_shutdown(&self) {
        self.cancel.cancelled().await;
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn socket(&self) -> &Arc<UdpSocket> {
        &self.socket
    }
}

impl Drop for RelayServer {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn run_loop(
    socket: Arc<UdpSocket>,
    cfg: RelayConfig,
    stats: Arc<RelayStats>,
    cancel: CancellationToken,
) {
    let mut peers: HashMap<NodeId, PeerEntry> = HashMap::new();
    let mut ip_buckets: HashMap<IpAddr, IpTokenBucket> = HashMap::new();
    // Allocate +1 byte so we can detect and drop datagrams exceeding `max_datagram_bytes`.
    let mut buf = vec![0u8; cfg.max_datagram_bytes.saturating_add(1)];
    let mut last_prune = Instant::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            recv = socket.recv_from(&mut buf) => {
                let (len, from_addr) = match recv {
                    Ok(pair) => pair,
                    Err(err) => {
                        tracing::debug!("Relay recv error: {}", err);
                        continue;
                    }
                };

                let now = Instant::now();
                if now.duration_since(last_prune) >= Duration::from_secs(15) {
                    peers.retain(|_, entry| now.duration_since(entry.last_seen) < cfg.registration_ttl);
                    ip_buckets.retain(|_, b| now.duration_since(b.last_refill) < Duration::from_secs(60));
                    last_prune = now;
                }

                // 1. Strict max datagram size enforcement before any decoding.
                if len == 0 || len > cfg.max_datagram_bytes {
                    stats.oversized_packets.fetch_add(1, Ordering::Relaxed);
                    continue;
                }

                // 2. Per-IP packet & byte token-bucket rate limit.
                let src_ip = from_addr.ip();
                if !ip_buckets.contains_key(&src_ip) && ip_buckets.len() >= cfg.max_tracked_ips {
                    // Evict stale IP entries first; if still at capacity, fail closed.
                    ip_buckets.retain(|_, b| now.duration_since(b.last_refill) < Duration::from_secs(10));
                    if ip_buckets.len() >= cfg.max_tracked_ips {
                        stats.rate_limited_packets.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                }
                let bucket = ip_buckets
                    .entry(src_ip)
                    .or_insert_with(|| IpTokenBucket::new(&cfg, now));
                if !bucket.allow_packet(&cfg, len, now) {
                    stats.rate_limited_packets.fetch_add(1, Ordering::Relaxed);
                    continue;
                }

                // 3. Decode signaling packet safely.
                let Ok(msg) = SignalingMessage::decode_datagram(&buf[..len]) else {
                    stats.malformed_packets.fetch_add(1, Ordering::Relaxed);
                    continue;
                };

                match msg {
                    SignalingMessage::StunRequest { tx_id } => {
                        if !bucket.allow_control(&cfg, now) {
                            stats.rate_limited_packets.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        stats.stun_requests.fetch_add(1, Ordering::Relaxed);
                        let resp = SignalingMessage::StunResponse {
                            tx_id,
                            observed_addr: from_addr,
                        };
                        if let Ok(encoded) = resp.encode_datagram() {
                            let _ = socket.send_to(&encoded, from_addr).await;
                        }
                    }
                    SignalingMessage::RelayRegister {
                        node_id,
                        mut local_addrs,
                    } => {
                        if !bucket.allow_control(&cfg, now) {
                            stats.rate_limited_packets.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        local_addrs.truncate(cfg.max_local_addrs_per_peer);
                        if !try_upsert_peer(&mut peers, node_id, from_addr, local_addrs, now, &cfg) {
                            stats.registration_cap_drops.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        stats.registrations.fetch_add(1, Ordering::Relaxed);
                        let resp = SignalingMessage::RelayRegistered {
                            observed_addr: from_addr,
                            ttl_secs: cfg.registration_ttl.as_secs() as u32,
                        };
                        if let Ok(encoded) = resp.encode_datagram() {
                            let _ = socket.send_to(&encoded, from_addr).await;
                        }
                    }
                    SignalingMessage::PeerLookup {
                        target_node_id,
                        requester_node_id,
                        mut requester_local_addrs,
                    } => {
                        if !bucket.allow_control(&cfg, now) {
                            stats.rate_limited_packets.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        requester_local_addrs.truncate(cfg.max_local_addrs_per_peer);
                        if !try_upsert_peer(
                            &mut peers,
                            requester_node_id,
                            from_addr,
                            requester_local_addrs.clone(),
                            now,
                            &cfg,
                        ) {
                            stats.registration_cap_drops.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        stats.lookups.fetch_add(1, Ordering::Relaxed);

                        let target_entry = peers.get(&target_node_id).and_then(|e| {
                            if now.duration_since(e.last_seen) < cfg.registration_ttl {
                                Some(e.clone())
                            } else {
                                None
                            }
                        });

                        let resp = SignalingMessage::PeerEndpoints {
                            target_node_id,
                            observed_addr: target_entry.as_ref().map(|e| e.observed_addr),
                            local_addrs: target_entry
                                .as_ref()
                                .map(|e| e.local_addrs.clone())
                                .unwrap_or_default(),
                        };
                        if let Ok(encoded) = resp.encode_datagram() {
                            let _ = socket.send_to(&encoded, from_addr).await;
                        }

                        if let Some(target) = target_entry {
                            let notify = SignalingMessage::PunchNotify {
                                from_node_id: requester_node_id,
                                from_observed_addr: from_addr,
                                from_local_addrs: requester_local_addrs,
                            };
                            if let Ok(encoded) = notify.encode_datagram() {
                                let _ = socket.send_to(&encoded, target.observed_addr).await;
                            }
                        }
                    }
                    SignalingMessage::RelayForward {
                        src_node_id,
                        dst_node_id,
                        payload,
                    } => {
                        if payload.is_empty() || payload.len() > cfg.max_relay_payload_bytes {
                            stats.oversized_packets.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        if !try_upsert_peer(&mut peers, src_node_id, from_addr, Vec::new(), now, &cfg) {
                            stats.registration_cap_drops.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }

                        if let Some(dst) = peers.get(&dst_node_id) {
                            if now.duration_since(dst.last_seen) < cfg.registration_ttl {
                                stats.forwarded_packets.fetch_add(1, Ordering::Relaxed);
                                stats
                                    .forwarded_bytes
                                    .fetch_add(payload.len() as u64, Ordering::Relaxed);
                                // Forward exact datagram blindly (end-to-end TLS 1.3 ciphertext).
                                let _ = socket.send_to(&buf[..len], dst.observed_addr).await;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Inserts or updates a peer entry while strictly enforcing global and per-IP registration caps.
fn try_upsert_peer(
    peers: &mut HashMap<NodeId, PeerEntry>,
    node_id: NodeId,
    observed_addr: SocketAddr,
    local_addrs: Vec<SocketAddr>,
    now: Instant,
    cfg: &RelayConfig,
) -> bool {
    if let Some(existing) = peers.get_mut(&node_id) {
        existing.observed_addr = observed_addr;
        if !local_addrs.is_empty() {
            existing.local_addrs = local_addrs;
        }
        existing.last_seen = now;
        return true;
    }

    // Evict expired peers if at global capacity before rejecting.
    if peers.len() >= cfg.max_total_peers {
        peers.retain(|_, e| now.duration_since(e.last_seen) < cfg.registration_ttl);
        if peers.len() >= cfg.max_total_peers {
            return false;
        }
    }

    let ip = observed_addr.ip();
    let count_for_ip = peers
        .values()
        .filter(|e| e.observed_addr.ip() == ip && now.duration_since(e.last_seen) < cfg.registration_ttl)
        .count();
    if count_for_ip >= cfg.max_peers_per_ip {
        return false;
    }

    peers.insert(
        node_id,
        PeerEntry {
            observed_addr,
            local_addrs,
            last_seen: now,
        },
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn relay_stun_registration_lookup_and_blind_forward() {
        let relay = RelayServer::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .unwrap();
        let relay_addr = relay.local_addr();

        let peer_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let peer_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let id_a = NodeId([0x11; 32]);
        let id_b = NodeId([0x22; 32]);

        let stun = SignalingMessage::StunRequest { tx_id: 42 }
            .encode_datagram()
            .unwrap();
        peer_a.send_to(&stun, relay_addr).await.unwrap();
        let mut buf = [0u8; 2048];
        let (n, _) = peer_a.recv_from(&mut buf).await.unwrap();
        match SignalingMessage::decode_datagram(&buf[..n]).unwrap() {
            SignalingMessage::StunResponse {
                tx_id,
                observed_addr,
            } => {
                assert_eq!(tx_id, 42);
                assert_eq!(observed_addr, peer_a.local_addr().unwrap());
            }
            other => panic!("unexpected STUN response: {other:?}"),
        }

        let reg_b = SignalingMessage::RelayRegister {
            node_id: id_b,
            local_addrs: vec![peer_b.local_addr().unwrap()],
        }
        .encode_datagram()
        .unwrap();
        peer_b.send_to(&reg_b, relay_addr).await.unwrap();
        let (n, _) = peer_b.recv_from(&mut buf).await.unwrap();
        assert!(matches!(
            SignalingMessage::decode_datagram(&buf[..n]).unwrap(),
            SignalingMessage::RelayRegistered { .. }
        ));

        let lookup = SignalingMessage::PeerLookup {
            target_node_id: id_b,
            requester_node_id: id_a,
            requester_local_addrs: vec![peer_a.local_addr().unwrap()],
        }
        .encode_datagram()
        .unwrap();
        peer_a.send_to(&lookup, relay_addr).await.unwrap();

        let (n_a, _) = peer_a.recv_from(&mut buf).await.unwrap();
        assert!(matches!(
            SignalingMessage::decode_datagram(&buf[..n_a]).unwrap(),
            SignalingMessage::PeerEndpoints {
                observed_addr: Some(_),
                ..
            }
        ));

        let (n_b, _) = peer_b.recv_from(&mut buf).await.unwrap();
        assert!(matches!(
            SignalingMessage::decode_datagram(&buf[..n_b]).unwrap(),
            SignalingMessage::PunchNotify {
                from_node_id,
                ..
            } if from_node_id == id_a
        ));

        let fwd = SignalingMessage::RelayForward {
            src_node_id: id_a,
            dst_node_id: id_b,
            payload: vec![0xDE, 0xAD, 0xBE, 0xEF],
        }
        .encode_datagram()
        .unwrap();
        peer_a.send_to(&fwd, relay_addr).await.unwrap();
        let (n_b, _) = peer_b.recv_from(&mut buf).await.unwrap();
        match SignalingMessage::decode_datagram(&buf[..n_b]).unwrap() {
            SignalingMessage::RelayForward {
                src_node_id,
                dst_node_id,
                payload,
            } => {
                assert_eq!(src_node_id, id_a);
                assert_eq!(dst_node_id, id_b);
                assert_eq!(payload, vec![0xDE, 0xAD, 0xBE, 0xEF]);
            }
            other => panic!("unexpected forwarded packet: {other:?}"),
        }

        assert_eq!(relay.stats().forwarded_packets.load(Ordering::Relaxed), 1);
        relay.shutdown();
    }

    #[tokio::test]
    async fn relay_enforces_rate_limits_oversized_drops_and_registration_caps() {
        let cfg = RelayConfig {
            max_datagram_bytes: 512,
            max_relay_payload_bytes: 256,
            max_packets_per_sec_per_ip: 100.0,
            packet_burst_per_ip: 100.0,
            max_bytes_per_sec_per_ip: 100_000.0,
            byte_burst_per_ip: 100_000.0,
            max_control_per_sec_per_ip: 1.0,
            control_burst_per_ip: 3.0,
            max_total_peers: 2,
            max_peers_per_ip: 2,
            max_local_addrs_per_peer: 2,
            ..Default::default()
        };

        let relay = RelayServer::bind_with_config(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            cfg,
        )
        .await
        .unwrap();
        let addr = relay.local_addr();
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        // 1. Oversized datagram (> 512 bytes) dropped immediately.
        let huge = vec![0x1Bu8; 700];
        sock.send_to(&huge, addr).await.unwrap();

        // 2. Malformed packet dropped cleanly.
        sock.send_to(b"not-veloce-garbage", addr).await.unwrap();

        // 3. Peer registration cap (max_peers_per_ip = 2): 3rd distinct NodeId from same IP dropped!
        for i in 1u8..=3 {
            let reg = SignalingMessage::RelayRegister {
                node_id: NodeId([i; 32]),
                local_addrs: vec![],
            }
            .encode_datagram()
            .unwrap();
            sock.send_to(&reg, addr).await.unwrap();
        }

        // 4. Control/STUN anti-amplification rate limit (burst = 3, already consumed 3 by register):
        //    Subsequent STUN requests must be rate-limited!
        for tx_id in 0..5u64 {
            let stun = SignalingMessage::StunRequest { tx_id }
                .encode_datagram()
                .unwrap();
            sock.send_to(&stun, addr).await.unwrap();
        }

        // 5. Oversized RelayForward payload (> 256 bytes, though total datagram < 512 bytes) dropped.
        let big_fwd = SignalingMessage::RelayForward {
            src_node_id: NodeId([1; 32]),
            dst_node_id: NodeId([2; 32]),
            payload: vec![0xAB; 300],
        }
        .encode_datagram()
        .unwrap();
        assert!(big_fwd.len() <= 512);
        sock.send_to(&big_fwd, addr).await.unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(relay.stats().oversized_packets.load(Ordering::Relaxed) >= 2);
        assert!(relay.stats().malformed_packets.load(Ordering::Relaxed) >= 1);
        assert_eq!(relay.stats().registrations.load(Ordering::Relaxed), 2);
        assert_eq!(relay.stats().registration_cap_drops.load(Ordering::Relaxed), 1);
        assert!(relay.stats().rate_limited_packets.load(Ordering::Relaxed) >= 4);
        assert_eq!(relay.stats().forwarded_packets.load(Ordering::Relaxed), 0);

        relay.shutdown();
    }
}
