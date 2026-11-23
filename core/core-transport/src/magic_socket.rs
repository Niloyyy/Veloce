use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io::{self, IoSliceMut};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::task::{ready, Context, Poll};

use core_protocol::{ConnectionType, NodeId, SignalingMessage, TransferProfile};
use quinn::udp::{RecvMeta, Transmit, UdpSocketState};
use quinn::{AsyncUdpSocket, UdpPoller};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::Interest;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc};

/// Virtual IPv4 prefix (RFC 2544 benchmark range `198.18.0.0/15`, never routed on public internet)
/// used internally so QUIC connections can migrate seamlessly between `Relayed` and `DirectP2p`
/// without tearing down streams or losing in-flight chunk state.
const VIRTUAL_IP_A: u8 = 198;
const VIRTUAL_IP_B: u8 = 18;
const VIRTUAL_PORT: u16 = 4433;

/// Maximum encapsulated QUIC datagram accepted inside a `RelayForward` signaling envelope.
const MAX_RELAY_FORWARD_PAYLOAD: usize = 2048;

/// Event emitted whenever a peer's active transport path changes (e.g., Relayed -> Direct-P2P).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathChangeEvent {
    pub peer_node_id: NodeId,
    pub virtual_addr: SocketAddr,
    pub connection_type: ConnectionType,
}

#[derive(Debug, Clone)]
struct PeerRoute {
    peer_node_id: NodeId,
    relay_addr: Option<SocketAddr>,
    direct_addr: Option<SocketAddr>,
    connection_type: ConnectionType,
}

#[derive(Default)]
struct RouteTable {
    by_virtual: HashMap<SocketAddr, PeerRoute>,
    node_to_virtual: HashMap<NodeId, SocketAddr>,
    direct_to_virtual: HashMap<SocketAddr, SocketAddr>,
}

/// Custom multiplexed UDP socket implementing `quinn::AsyncUdpSocket`.
///
/// Optimized for desktop gigabit LANs while gracefully degrading on mobile (Android/iOS) kernels:
/// - Requests profile-appropriate `SO_RCVBUF`/`SO_SNDBUF` sizes via `socket2` with step-down fallback
/// - Uses kernel UDP GSO/GRO (`UdpSocketState`) when supported and automatically latches
///   `gso_disabled` / standard `try_send_to` + `try_recv_from` fallback if rejected by Android/iOS
/// - Lock-free `AtomicBool` fast-path guards avoid `RwLock` acquisition on clean LAN traffic
/// - Shares one UDP port for QUIC, STUN/hole-punching (`0x1BVLC`), and blind encrypted `RelayForward`
pub struct MagicUdpSocket {
    io: Arc<UdpSocket>,
    /// Optional kernel offload state (`None` when OS rejects GSO/GRO socket options).
    udp_state: Option<UdpSocketState>,
    /// Latched to `true` if a mobile/VPN kernel rejects `UDP_SEGMENT` (GSO) at send time.
    gso_disabled: AtomicBool,
    local_node_id: NodeId,
    routes: RwLock<RouteTable>,
    /// Lock-free fast-path flag: false until at least one virtual route is installed.
    has_virtual_routes: AtomicBool,
    /// Lock-free fast-path flag: false until at least one direct->virtual override exists.
    has_direct_overrides: AtomicBool,
    next_virtual_id: AtomicU32,
    signaling_tx: mpsc::UnboundedSender<(SocketAddr, SignalingMessage)>,
    path_events_tx: broadcast::Sender<PathChangeEvent>,
    /// Diagnostic / test knob forcing traffic for virtual routes through the relay.
    force_relay: AtomicBool,
    configured_recv_buf: usize,
    configured_send_buf: usize,
}

impl fmt::Debug for MagicUdpSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MagicUdpSocket")
            .field("local_node_id", &self.local_node_id)
            .field("local_addr", &self.io.local_addr().ok())
            .field("recv_buf", &self.configured_recv_buf)
            .field("send_buf", &self.configured_send_buf)
            .field("gso_enabled", &self.is_gso_enabled())
            .finish()
    }
}

impl MagicUdpSocket {
    /// Binds a tuned UDP socket using the default Desktop profile.
    pub async fn bind(
        bind_addr: SocketAddr,
        local_node_id: NodeId,
    ) -> io::Result<(
        Arc<Self>,
        mpsc::UnboundedReceiver<(SocketAddr, SignalingMessage)>,
    )> {
        Self::bind_with_profile(bind_addr, local_node_id, TransferProfile::Desktop).await
    }

    /// Binds a UDP socket tuned for `TransferProfile` (`Desktop` vs `Mobile`), degrading
    /// gracefully if OS socket buffer sizes or kernel GSO/GRO options are restricted.
    pub async fn bind_with_profile(
        bind_addr: SocketAddr,
        local_node_id: NodeId,
        profile: TransferProfile,
    ) -> io::Result<(
        Arc<Self>,
        mpsc::UnboundedReceiver<(SocketAddr, SignalingMessage)>,
    )> {
        let (std_sock, recv_buf, send_buf) =
            bind_tuned_udp_socket(bind_addr, profile.udp_buffer_ladder())?;
        let udp_state = match UdpSocketState::new((&std_sock).into()) {
            Ok(state) => Some(state),
            Err(err) => {
                tracing::warn!(
                    "UDP GSO/GRO socket state unavailable on this platform ({}); falling back to standard UDP I/O",
                    err
                );
                None
            }
        };
        let io = Arc::new(UdpSocket::from_std(std_sock)?);
        let (signaling_tx, signaling_rx) = mpsc::unbounded_channel();
        let (path_events_tx, _) = broadcast::channel(64);
        let gso_disabled = udp_state.is_none();

        let sock = Arc::new(Self {
            io,
            udp_state,
            gso_disabled: AtomicBool::new(gso_disabled),
            local_node_id,
            routes: RwLock::new(RouteTable::default()),
            has_virtual_routes: AtomicBool::new(false),
            has_direct_overrides: AtomicBool::new(false),
            next_virtual_id: AtomicU32::new(1),
            signaling_tx,
            path_events_tx,
            force_relay: AtomicBool::new(false),
            configured_recv_buf: recv_buf,
            configured_send_buf: send_buf,
        });
        Ok((sock, signaling_rx))
    }

    pub fn local_node_id(&self) -> NodeId {
        self.local_node_id
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.io.local_addr()
    }

    pub fn configured_buffer_sizes(&self) -> (usize, usize) {
        (self.configured_recv_buf, self.configured_send_buf)
    }

    pub fn is_gso_enabled(&self) -> bool {
        self.udp_state.is_some() && !self.gso_disabled.load(Ordering::Acquire)
    }

    /// Disables GSO (testing/diagnostic helper verifying mobile kernel fallback behavior).
    pub fn force_disable_gso(&self) {
        self.gso_disabled.store(true, Ordering::Release);
    }

    pub fn set_force_relay(&self, force: bool) {
        self.force_relay.store(force, Ordering::Relaxed);
    }

    pub fn is_force_relay(&self) -> bool {
        self.force_relay.load(Ordering::Relaxed)
    }

    pub fn subscribe_path_changes(&self) -> broadcast::Receiver<PathChangeEvent> {
        self.path_events_tx.subscribe()
    }

    /// Sends a Veloce signaling / STUN / hole-punch UDP datagram on the shared socket.
    pub async fn send_signaling(
        &self,
        dest: SocketAddr,
        msg: &SignalingMessage,
    ) -> io::Result<()> {
        let bytes = msg
            .encode_datagram()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        self.io.send_to(&bytes, dest).await?;
        Ok(())
    }

    /// Ensures a virtual route exists for `peer_node_id` and returns its virtual `SocketAddr`.
    pub fn ensure_virtual_route(
        &self,
        peer_node_id: NodeId,
        relay_addr: Option<SocketAddr>,
        direct_addr: Option<SocketAddr>,
        initial_type: ConnectionType,
    ) -> SocketAddr {
        let mut table = self.routes.write().expect("route table lock poisoned");
        if let Some(&existing_virt) = table.node_to_virtual.get(&peer_node_id) {
            if let Some(route) = table.by_virtual.get_mut(&existing_virt) {
                if relay_addr.is_some() {
                    route.relay_addr = relay_addr;
                }
                if let Some(dir) = direct_addr {
                    if !self.force_relay.load(Ordering::Relaxed) {
                        route.direct_addr = Some(dir);
                        route.connection_type = initial_type;
                        table.direct_to_virtual.insert(dir, existing_virt);
                        self.has_direct_overrides.store(true, Ordering::Release);
                    }
                }
            }
            return existing_virt;
        }

        let seq = self.next_virtual_id.fetch_add(1, Ordering::Relaxed);
        let oct3 = ((seq >> 8) & 0xFF) as u8;
        let oct4 = (seq & 0xFF).max(1) as u8;
        let virtual_addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(VIRTUAL_IP_A, VIRTUAL_IP_B, oct3, oct4)),
            VIRTUAL_PORT,
        );

        let effective_direct = if self.force_relay.load(Ordering::Relaxed) {
            None
        } else {
            direct_addr
        };
        let effective_type = if effective_direct.is_some() {
            initial_type
        } else {
            ConnectionType::Relayed
        };

        let route = PeerRoute {
            peer_node_id,
            relay_addr,
            direct_addr: effective_direct,
            connection_type: effective_type,
        };
        table.node_to_virtual.insert(peer_node_id, virtual_addr);
        if let Some(dir) = effective_direct {
            table.direct_to_virtual.insert(dir, virtual_addr);
            self.has_direct_overrides.store(true, Ordering::Release);
        }
        table.by_virtual.insert(virtual_addr, route);
        self.has_virtual_routes.store(true, Ordering::Release);
        virtual_addr
    }

    /// Atomically upgrades an existing peer route (e.g., from `Relayed` to `DirectP2p`)
    /// without interrupting any active QUIC connection on that peer's virtual address.
    pub fn upgrade_peer_to_direct(
        &self,
        peer_node_id: NodeId,
        direct_addr: SocketAddr,
        connection_type: ConnectionType,
    ) -> bool {
        if self.force_relay.load(Ordering::Relaxed) {
            return false;
        }
        let mut table = self.routes.write().expect("route table lock poisoned");
        let Some(&virtual_addr) = table.node_to_virtual.get(&peer_node_id) else {
            return false;
        };
        let changed = if let Some(route) = table.by_virtual.get_mut(&virtual_addr) {
            let was_different =
                route.direct_addr != Some(direct_addr) || route.connection_type != connection_type;
            route.direct_addr = Some(direct_addr);
            route.connection_type = connection_type;
            was_different
        } else {
            false
        };
        table.direct_to_virtual.insert(direct_addr, virtual_addr);
        self.has_direct_overrides.store(true, Ordering::Release);
        drop(table);

        if changed {
            let _ = self.path_events_tx.send(PathChangeEvent {
                peer_node_id,
                virtual_addr,
                connection_type,
            });
        }
        changed
    }

    /// Returns the active `ConnectionType` for a peer if routed through a virtual address.
    pub fn connection_type_for_addr(&self, addr: SocketAddr) -> Option<ConnectionType> {
        if !self.has_virtual_routes.load(Ordering::Acquire) {
            return None;
        }
        let table = self.routes.read().expect("route table lock poisoned");
        table.by_virtual.get(&addr).map(|r| r.connection_type)
    }

    /// Returns the active `ConnectionType` for a known `NodeId`.
    pub fn connection_type_for_peer(&self, peer_node_id: &NodeId) -> Option<ConnectionType> {
        if !self.has_virtual_routes.load(Ordering::Acquire) {
            return None;
        }
        let table = self.routes.read().expect("route table lock poisoned");
        let virt = table.node_to_virtual.get(peer_node_id)?;
        table.by_virtual.get(virt).map(|r| r.connection_type)
    }

    /// Checks whether a `SocketAddr` is a virtual route allocated by `MagicUdpSocket`.
    #[inline]
    pub fn is_virtual_addr(addr: SocketAddr) -> bool {
        match addr.ip() {
            IpAddr::V4(v4) => {
                let octs = v4.octets();
                octs[0] == VIRTUAL_IP_A && octs[1] == VIRTUAL_IP_B
            }
            IpAddr::V6(_) => false,
        }
    }

    /// Sends a direct QUIC transmit using kernel GSO when enabled, automatically latching
    /// `gso_disabled` and falling back to segmented `try_send_to` if rejected by Android/iOS.
    fn send_direct(&self, transmit: &Transmit) -> io::Result<()> {
        if let Some(ref state) = self.udp_state {
            if !self.gso_disabled.load(Ordering::Acquire) {
                match self.io.try_io(Interest::WRITABLE, || {
                    state.send((&*self.io).into(), transmit)
                }) {
                    Ok(()) => return Ok(()),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Err(e),
                    Err(err) if transmit.segment_size.is_some() => {
                        tracing::warn!(
                            "Kernel UDP GSO rejected ({}); disabling GSO and falling back to single-segment UDP sends",
                            err
                        );
                        self.gso_disabled.store(true, Ordering::Release);
                    }
                    Err(err) => return Err(err),
                }
            }
        }

        let seg_size = transmit
            .segment_size
            .unwrap_or(transmit.contents.len())
            .max(1);
        for segment in transmit.contents.chunks(seg_size) {
            self.io.try_send_to(segment, transmit.destination)?;
        }
        Ok(())
    }
}

/// Creates a non-blocking UDP socket with profile-appropriate `SO_RCVBUF`/`SO_SNDBUF` sizes,
/// gracefully stepping down if restricted by mobile/container kernel caps.
fn bind_tuned_udp_socket(
    bind_addr: SocketAddr,
    buffer_ladder: &[usize],
) -> io::Result<(std::net::UdpSocket, usize, usize)> {
    let domain = Domain::for_address(bind_addr);
    let sock = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_nonblocking(true)?;

    for &size in buffer_ladder {
        if sock.set_recv_buffer_size(size).is_ok() {
            break;
        }
    }
    for &size in buffer_ladder {
        if sock.set_send_buffer_size(size).is_ok() {
            break;
        }
    }

    sock.bind(&bind_addr.into())?;
    let actual_recv = sock.recv_buffer_size().unwrap_or(0);
    let actual_send = sock.send_buffer_size().unwrap_or(0);
    Ok((sock.into(), actual_recv, actual_send))
}

impl AsyncUdpSocket for MagicUdpSocket {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        Box::pin(MagicUdpPoller {
            io: Arc::clone(&self.io),
            fut: Mutex::new(None),
        })
    }

    fn try_send(&self, transmit: &Transmit) -> io::Result<()> {
        let dest = transmit.destination;

        // Lock-free hot path for direct LAN / direct WAN UDP packets.
        if !self.has_virtual_routes.load(Ordering::Acquire) || !Self::is_virtual_addr(dest) {
            return self.send_direct(transmit);
        }

        let route = {
            let table = self.routes.read().expect("route table lock poisoned");
            table.by_virtual.get(&dest).cloned()
        };
        let Some(route) = route else {
            return Ok(());
        };

        let seg_size = transmit
            .segment_size
            .unwrap_or(transmit.contents.len())
            .max(1);

        if !self.force_relay.load(Ordering::Relaxed) {
            if let Some(direct) = route.direct_addr {
                let rewritten = Transmit {
                    destination: direct,
                    ecn: transmit.ecn,
                    contents: transmit.contents,
                    segment_size: transmit.segment_size,
                    src_ip: transmit.src_ip,
                };
                return self.send_direct(&rewritten);
            }
        }

        if let Some(relay_addr) = route.relay_addr {
            for segment in transmit.contents.chunks(seg_size) {
                let wrapped = SignalingMessage::RelayForward {
                    src_node_id: self.local_node_id,
                    dst_node_id: route.peer_node_id,
                    payload: segment.to_vec(),
                }
                .encode_datagram()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                self.io.try_send_to(&wrapped, relay_addr)?;
            }
        }

        Ok(())
    }

    fn poll_recv(
        &self,
        cx: &mut Context,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        let capacity = bufs.len().min(meta.len());
        if capacity == 0 {
            return Poll::Ready(Ok(0));
        }

        loop {
            ready!(self.io.poll_recv_ready(cx))?;
            let count = if let Some(ref state) = self.udp_state {
                match self.io.try_io(Interest::READABLE, || {
                    state.recv((&*self.io).into(), &mut bufs[..capacity], &mut meta[..capacity])
                }) {
                    Ok(n) => n,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(e) => return Poll::Ready(Err(e)),
                }
            } else {
                // Fallback path when UdpSocketState (GRO/recvmmsg) is unavailable on the platform.
                let mut n_read = 0usize;
                while n_read < capacity {
                    match self.io.try_recv_from(&mut bufs[n_read]) {
                        Ok((len, addr)) => {
                            meta[n_read] = RecvMeta {
                                addr,
                                len,
                                stride: len,
                                ecn: None,
                                dst_ip: None,
                            };
                            n_read += 1;
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) if n_read > 0 => {
                            let _ = e;
                            break;
                        }
                        Err(e) => return Poll::Ready(Err(e)),
                    }
                }
                if n_read == 0 {
                    continue;
                }
                n_read
            };

            let mut write_idx = 0usize;
            let has_overrides = self.has_direct_overrides.load(Ordering::Acquire);

            for read_idx in 0..count {
                let m = meta[read_idx];
                if m.len == 0 {
                    continue;
                }

                let is_sig = m.len == m.stride
                    && SignalingMessage::is_signaling_datagram(&bufs[read_idx][..m.len]);

                if is_sig {
                    let Ok(msg) = SignalingMessage::decode_datagram(&bufs[read_idx][..m.len]) else {
                        continue;
                    };
                    match msg {
                        SignalingMessage::RelayForward {
                            src_node_id,
                            payload,
                            ..
                        } => {
                            if payload.is_empty()
                                || payload.len() > MAX_RELAY_FORWARD_PAYLOAD
                                || payload.len() > bufs[write_idx].len()
                            {
                                continue;
                            }
                            let virt_addr = self.ensure_virtual_route(
                                src_node_id,
                                Some(m.addr),
                                None,
                                ConnectionType::Relayed,
                            );
                            bufs[write_idx][..payload.len()].copy_from_slice(&payload);
                            meta[write_idx] = RecvMeta {
                                addr: virt_addr,
                                len: payload.len(),
                                stride: payload.len(),
                                ecn: None,
                                dst_ip: None,
                            };
                            write_idx += 1;
                        }
                        other => {
                            let _ = self.signaling_tx.send((m.addr, other));
                        }
                    }
                    continue;
                }

                let mapped_addr = if has_overrides {
                    let table = self.routes.read().expect("route table lock poisoned");
                    table
                        .direct_to_virtual
                        .get(&m.addr)
                        .copied()
                        .unwrap_or(m.addr)
                } else {
                    m.addr
                };

                if write_idx != read_idx {
                    let copy_len = m.len.min(bufs[write_idx].len());
                    let (left, right) = bufs.split_at_mut(read_idx);
                    left[write_idx][..copy_len].copy_from_slice(&right[0][..copy_len]);
                }

                meta[write_idx] = RecvMeta {
                    addr: mapped_addr,
                    len: m.len,
                    stride: m.stride,
                    ecn: m.ecn,
                    dst_ip: m.dst_ip,
                };
                write_idx += 1;
            }

            if write_idx > 0 {
                return Poll::Ready(Ok(write_idx));
            }
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.io.local_addr()
    }

    fn may_fragment(&self) -> bool {
        true
    }

    fn max_transmit_segments(&self) -> usize {
        if self.gso_disabled.load(Ordering::Acquire) {
            return 1;
        }
        self.udp_state
            .as_ref()
            .map(UdpSocketState::max_gso_segments)
            .unwrap_or(1)
    }

    fn max_receive_segments(&self) -> usize {
        self.udp_state
            .as_ref()
            .map(UdpSocketState::gro_segments)
            .unwrap_or(1)
    }
}

type WritableFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

struct MagicUdpPoller {
    io: Arc<UdpSocket>,
    fut: Mutex<Option<WritableFuture>>,
}

impl fmt::Debug for MagicUdpPoller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MagicUdpPoller").finish()
    }
}

impl UdpPoller for MagicUdpPoller {
    fn poll_writable(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let mut guard = self.fut.lock().expect("poller mutex poisoned");
        let fut = guard.get_or_insert_with(|| {
            let io = Arc::clone(&self.io);
            Box::pin(async move { io.writable().await })
        });
        match fut.as_mut().poll(cx) {
            Poll::Ready(res) => {
                *guard = None;
                Poll::Ready(res)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
