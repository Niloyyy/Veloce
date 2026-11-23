use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use core_protocol::{
    read_frame, write_frame, ConnectionType, ControlMessage, HandshakeStateMachine, NodeId,
    SessionRole, SessionTicket, TransferManifest, TransferProfile, MAX_CONTROL_FRAME_SIZE,
    PROTOCOL_VERSION,
};
use core_storage::ChunkSource;
use quinn::{Endpoint, EndpointConfig, TransportConfig};
use rand::RngCore;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::discovery::{
    local_lan_addrs_with_overrides, DiscoveryEvent, LanDiscovery, LocalPeer,
};
use crate::error::{Result, TransportError};
use crate::identity::{build_bbr_transport_config_for_profile, peer_node_id, PeerIdentity};
use crate::magic_socket::{MagicUdpSocket, PathChangeEvent};
use crate::nat::NatCoordinator;
use crate::transfer::{
    graceful_close, run_receiver_session_with_profile, run_sender_session, send_and_receive_hello,
    TransferProgress, TransferSummary, DEFAULT_PARALLEL_STREAMS,
};

/// Decision returned by the application / UI for an incoming LAN push transfer offer.
#[derive(Debug)]
pub enum IncomingOfferDecision {
    Accept { output_dir: PathBuf },
    Reject { reason: String },
}

/// Incoming LAN transfer offer awaiting user approval.
#[derive(Debug)]
pub struct IncomingOffer {
    pub transfer_id: String,
    pub sender_node_id: NodeId,
    pub sender_name: String,
    pub manifest: TransferManifest,
    pub connection_type: ConnectionType,
    pub responder: oneshot::Sender<IncomingOfferDecision>,
}

/// Registered hosted session waiting for remote ticket holders to connect and pull chunks.
#[derive(Clone)]
pub struct HostedSession {
    pub transfer_id: String,
    pub session_token: [u8; 16],
    pub ticket: SessionTicket,
    pub ticket_code: String,
    pub source: Arc<ChunkSource>,
    pub paused: Arc<AtomicBool>,
    pub cancel: CancellationToken,
}

/// Event emitted when a transfer's underlying transport path transitions (e.g., Relayed -> Direct-P2P).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferConnectionChanged {
    pub transfer_id: String,
    pub peer_node_id: NodeId,
    pub connection_type: ConnectionType,
}

/// High-level QUIC P2P endpoint handling BBR multi-stream transfers, mDNS LAN discovery,
/// STUN/UDP hole-punching, and end-to-end encrypted relay fallback with live path upgrades.
pub struct VeloceEndpoint {
    identity: PeerIdentity,
    device_name: String,
    profile: TransferProfile,
    override_lan_addrs: Vec<IpAddr>,
    relay_hosts: Vec<String>,
    endpoint: Endpoint,
    magic_socket: Arc<MagicUdpSocket>,
    nat: Arc<NatCoordinator>,
    transport_config: Arc<TransportConfig>,
    listen_addr: SocketAddr,
    discovery: Option<LanDiscovery>,
    hosted_sessions: Arc<RwLock<HashMap<[u8; 16], HostedSession>>>,
    active_cancellations: Arc<Mutex<HashMap<String, CancellationToken>>>,
    progress_tx: mpsc::UnboundedSender<TransferProgress>,
    conn_changed_tx: broadcast::Sender<TransferConnectionChanged>,
    default_parallel_streams: usize,
    cancel: CancellationToken,
}

/// Configuration for spinning up a `VeloceEndpoint`.
#[derive(Debug, Clone)]
pub struct EndpointOptions {
    pub bind_addr: SocketAddr,
    pub device_name: String,
    pub enable_mdns: bool,
    pub parallel_streams: usize,
    /// Pre-resolved relay socket addresses.
    pub relay_addrs: Vec<SocketAddr>,
    /// Relay endpoints specified as DNS `host:port` or `ip:port` strings (resolved dynamically).
    pub relay_hosts: Vec<String>,
    /// Runtime memory & concurrency profile (`Desktop` vs `Mobile`).
    pub profile: TransferProfile,
    /// Optional platform-provided local LAN IPs (e.g., Android API 30+ `ConnectivityManager`).
    pub override_lan_addrs: Vec<IpAddr>,
    /// Forces virtual peer traffic through the relay (useful for symmetric NAT testing).
    pub force_relay: bool,
}

impl Default for EndpointOptions {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            device_name: "Veloce Node".into(),
            enable_mdns: true,
            parallel_streams: DEFAULT_PARALLEL_STREAMS,
            relay_addrs: Vec::new(),
            relay_hosts: Vec::new(),
            profile: TransferProfile::Desktop,
            override_lan_addrs: Vec::new(),
            force_relay: false,
        }
    }
}

/// Receiver channels returned alongside a running `VeloceEndpoint`.
pub struct EndpointChannels {
    pub incoming_offers: mpsc::UnboundedReceiver<IncomingOffer>,
    pub progress_rx: mpsc::UnboundedReceiver<TransferProgress>,
    pub conn_changed_rx: broadcast::Receiver<TransferConnectionChanged>,
}

impl VeloceEndpoint {
    /// Binds a multiplexed `MagicUdpSocket` under QUIC with BBR congestion control,
    /// Ed25519 mutual TLS 1.3, optional mDNS discovery, and NAT/relay coordination.
    pub async fn bind(opts: EndpointOptions) -> Result<(Arc<Self>, EndpointChannels)> {
        let identity = PeerIdentity::generate()?;
        let transport_config = build_bbr_transport_config_for_profile(opts.profile)?;
        let server_config = identity.build_server_config(Arc::clone(&transport_config))?;

        let (magic_socket, signaling_rx) =
            MagicUdpSocket::bind_with_profile(opts.bind_addr, identity.node_id(), opts.profile)
                .await?;
        magic_socket.set_force_relay(opts.force_relay);
        let listen_addr = magic_socket.local_addr()?;

        let runtime = quinn::default_runtime().ok_or_else(|| {
            TransportError::Connection("no async runtime available for QUIC endpoint".into())
        })?;
        let endpoint = Endpoint::new_with_abstract_socket(
            EndpointConfig::default(),
            Some(server_config),
            Arc::clone(&magic_socket) as Arc<dyn quinn::AsyncUdpSocket>,
            runtime,
        )?;

        // Resolve any DNS `host:port` relay entries alongside pre-resolved `relay_addrs`.
        let mut resolved_relays = opts.relay_addrs.clone();
        for host in &opts.relay_hosts {
            let trimmed = host.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(iter) = tokio::net::lookup_host(trimmed).await {
                for addr in iter {
                    if !resolved_relays.contains(&addr) {
                        resolved_relays.push(addr);
                    }
                }
            }
        }

        let cancel = CancellationToken::new();
        let nat = NatCoordinator::start(
            Arc::clone(&magic_socket),
            signaling_rx,
            resolved_relays.clone(),
            cancel.clone(),
        );

        // Eagerly register with configured relays so incoming global sessions can reach us immediately.
        for relay in &resolved_relays {
            let _ = nat.register_with_relay(*relay).await;
        }

        let discovery = if opts.enable_mdns {
            match LanDiscovery::start_with_overrides(
                identity.node_id(),
                opts.device_name.clone(),
                listen_addr.port(),
                &opts.override_lan_addrs,
            ) {
                Ok(disc) => Some(disc),
                Err(err) => {
                    tracing::warn!("mDNS LAN discovery unavailable: {}", err);
                    None
                }
            }
        } else {
            None
        };

        let (offers_tx, incoming_offers) = mpsc::unbounded_channel();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        let (conn_changed_tx, conn_changed_rx) = broadcast::channel(64);
        let hosted_sessions: Arc<RwLock<HashMap<[u8; 16], HostedSession>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let active_cancellations: Arc<Mutex<HashMap<String, CancellationToken>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let node = Arc::new(Self {
            identity,
            device_name: opts.device_name,
            profile: opts.profile,
            override_lan_addrs: opts.override_lan_addrs,
            relay_hosts: opts.relay_hosts,
            endpoint,
            magic_socket,
            nat,
            transport_config,
            listen_addr,
            discovery,
            hosted_sessions,
            active_cancellations,
            progress_tx,
            conn_changed_tx,
            default_parallel_streams: opts.parallel_streams.max(1),
            cancel,
        });

        node.spawn_accept_loop(offers_tx, node.default_parallel_streams);

        Ok((
            node,
            EndpointChannels {
                incoming_offers,
                progress_rx,
                conn_changed_rx,
            },
        ))
    }

    pub fn node_id(&self) -> NodeId {
        self.identity.node_id()
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn profile(&self) -> TransferProfile {
        self.profile
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn magic_socket(&self) -> &Arc<MagicUdpSocket> {
        &self.magic_socket
    }

    pub fn nat(&self) -> &Arc<NatCoordinator> {
        &self.nat
    }

    pub fn set_force_relay(&self, force: bool) {
        self.magic_socket.set_force_relay(force);
    }

    pub fn local_peers(&self) -> Vec<LocalPeer> {
        self.discovery
            .as_ref()
            .map(LanDiscovery::list_peers)
            .unwrap_or_default()
    }

    pub fn subscribe_discovery(&self) -> Option<broadcast::Receiver<DiscoveryEvent>> {
        self.discovery.as_ref().map(LanDiscovery::subscribe)
    }

    pub fn subscribe_path_changes(&self) -> broadcast::Receiver<PathChangeEvent> {
        self.magic_socket.subscribe_path_changes()
    }

    pub fn subscribe_connection_changes(&self) -> broadcast::Receiver<TransferConnectionChanged> {
        self.conn_changed_tx.subscribe()
    }

    /// Registers files for sharing and generates a shareable `SessionTicket` (plus `veloce1...` code)
    /// containing LAN hints, STUN reflexive public address (if discovered), and relay hints.
    pub fn host_session(
        &self,
        source: Arc<ChunkSource>,
        extra_addrs: Vec<SocketAddr>,
        mut relay_hints: Vec<String>,
    ) -> Result<HostedSession> {
        let mut session_token = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut session_token);

        let mut direct_addrs =
            local_lan_addrs_with_overrides(self.listen_addr.port(), &self.override_lan_addrs);
        if let Some(public) = self.nat.public_addr() {
            if !direct_addrs.contains(&public) {
                direct_addrs.push(public);
            }
        }
        for addr in extra_addrs {
            if !direct_addrs.contains(&addr) {
                direct_addrs.push(addr);
            }
        }
        if direct_addrs.is_empty() {
            direct_addrs.push(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                self.listen_addr.port(),
            ));
        }

        // Preserve DNS `host:port` relay strings first so tickets remain dual-stack portable.
        for host in &self.relay_hosts {
            let trimmed = host.trim().to_string();
            if !trimmed.is_empty() && !relay_hints.contains(&trimmed) {
                relay_hints.push(trimmed);
            }
        }
        for configured_relay in self.nat.relay_addrs() {
            let s = configured_relay.to_string();
            if !relay_hints.contains(&s) {
                relay_hints.push(s);
            }
        }

        let manifest = source.manifest();
        let ticket = SessionTicket::new(
            self.node_id(),
            direct_addrs,
            relay_hints,
            manifest.merkle_root,
            session_token,
            manifest.total_size,
            manifest.files.len() as u32,
        );
        let ticket_code = ticket.encode()?;
        let transfer_id = hex::encode(&manifest.session_id[..8]);
        let cancel = self.cancel.child_token();

        let hosted = HostedSession {
            transfer_id: transfer_id.clone(),
            session_token,
            ticket,
            ticket_code,
            source,
            paused: Arc::new(AtomicBool::new(false)),
            cancel: cancel.clone(),
        };

        self.hosted_sessions
            .write()
            .expect("hosted_sessions lock poisoned")
            .insert(session_token, hosted.clone());
        self.active_cancellations
            .lock()
            .expect("cancellations lock poisoned")
            .insert(transfer_id, cancel);

        Ok(hosted)
    }

    /// Smart global session joiner:
    /// 1. Tries direct LAN / mDNS addresses first (unless `force_relay` is enabled).
    /// 2. Coordinates STUN + simultaneous UDP hole-punching via relay hints.
    /// 3. Falls back seamlessly to end-to-end encrypted relay while continuously probing
    ///    in the background to live-upgrade the active QUIC connection back to `Direct-P2P`.
    pub async fn join_session(
        &self,
        ticket: &SessionTicket,
        output_dir: PathBuf,
        parallel_streams: Option<usize>,
    ) -> Result<TransferSummary> {
        let streams = parallel_streams.unwrap_or(self.default_parallel_streams);
        let transfer_id = hex::encode(&ticket.merkle_root[..8]);

        // Resolve relay hints from ticket + configured DNS relay_hosts + pre-resolved relays.
        let mut relays: Vec<SocketAddr> = Vec::new();
        for hint in ticket.relay_hints.iter().chain(self.relay_hosts.iter()) {
            let trimmed = hint.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(resolved) = tokio::net::lookup_host(trimmed).await {
                for addr in resolved {
                    if !relays.contains(&addr) {
                        relays.push(addr);
                    }
                }
            }
        }
        for r in self.nat.relay_addrs() {
            if !relays.contains(&r) {
                relays.push(r);
            }
        }

        // Gather direct candidate addresses (mDNS peers first, then ticket hints).
        let mut direct_candidates: Vec<SocketAddr> = Vec::new();
        if let Some(ref disc) = self.discovery {
            if let Some(peer) = disc.get_peer(&ticket.node_id) {
                for addr in peer.addresses {
                    if !direct_candidates.contains(&addr) {
                        direct_candidates.push(addr);
                    }
                }
            }
        }
        for &addr in &ticket.direct_addrs {
            if !direct_candidates.contains(&addr) {
                direct_candidates.push(addr);
            }
        }

        // Step 1: Attempt direct LAN/WAN connection first unless force_relay is set.
        if !self.magic_socket.is_force_relay() {
            let direct_timeout = if relays.is_empty() {
                Duration::from_secs(6)
            } else {
                Duration::from_millis(1200)
            };
            for &addr in &direct_candidates {
                if let Ok(conn) = self
                    .connect_verified_with_timeout(addr, Some(ticket.node_id), direct_timeout)
                    .await
                {
                    let conn_type = classify_socket_addr(addr);
                    let _ = self.conn_changed_tx.send(TransferConnectionChanged {
                        transfer_id: transfer_id.clone(),
                        peer_node_id: ticket.node_id,
                        connection_type: conn_type,
                    });
                    return self
                        .pull_over_connection(
                            transfer_id,
                            ticket,
                            conn,
                            output_dir,
                            streams,
                            conn_type,
                            Vec::new(),
                        )
                        .await;
                }
            }
        }

        // Step 2 & 3: NAT traversal via relay (UDP hole-punching -> encrypted relay fallback).
        let mut last_err: Option<TransportError> = None;
        for relay_addr in relays {
            match self
                .nat
                .resolve_peer_via_relay(
                    ticket.node_id,
                    relay_addr,
                    &direct_candidates,
                    Duration::from_millis(800),
                )
                .await
            {
                Ok(plan) => {
                    match self
                        .connect_verified_with_timeout(
                            plan.dial_addr,
                            Some(ticket.node_id),
                            Duration::from_secs(10),
                        )
                        .await
                    {
                        Ok(conn) => {
                            let _ = self.conn_changed_tx.send(TransferConnectionChanged {
                                transfer_id: transfer_id.clone(),
                                peer_node_id: ticket.node_id,
                                connection_type: plan.connection_type,
                            });
                            return self
                                .pull_over_connection(
                                    transfer_id,
                                    ticket,
                                    conn,
                                    output_dir,
                                    streams,
                                    plan.connection_type,
                                    plan.upgrade_candidates,
                                )
                                .await;
                        }
                        Err(err) => {
                            last_err = Some(err);
                        }
                    }
                }
                Err(err) => {
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            TransportError::NatTraversal(
                "could not reach peer directly or via any configured relay".into(),
            )
        }))
    }

    /// Directly connects to a known socket address and pulls a session described by a `SessionTicket`.
    pub async fn pull_session_direct(
        &self,
        ticket: &SessionTicket,
        target_addr: SocketAddr,
        output_dir: PathBuf,
        parallel_streams: usize,
        connection_type: ConnectionType,
    ) -> Result<TransferSummary> {
        let transfer_id = hex::encode(&ticket.merkle_root[..8]);
        let conn = self
            .connect_verified_with_timeout(target_addr, Some(ticket.node_id), Duration::from_secs(15))
            .await?;
        let _ = self.conn_changed_tx.send(TransferConnectionChanged {
            transfer_id: transfer_id.clone(),
            peer_node_id: ticket.node_id,
            connection_type,
        });
        self.pull_over_connection(
            transfer_id,
            ticket,
            conn,
            output_dir,
            parallel_streams,
            connection_type,
            Vec::new(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn pull_over_connection(
        &self,
        transfer_id: String,
        ticket: &SessionTicket,
        conn: quinn::Connection,
        output_dir: PathBuf,
        parallel_streams: usize,
        initial_connection_type: ConnectionType,
        upgrade_candidates: Vec<SocketAddr>,
    ) -> Result<TransferSummary> {
        let cancel = self.cancel.child_token();
        self.active_cancellations
            .lock()
            .expect("cancellations lock poisoned")
            .insert(transfer_id.clone(), cancel.clone());

        let (mut ctrl_send, mut ctrl_recv) = conn.open_bi().await?;
        let mut sm = HandshakeStateMachine::new(SessionRole::Receiver, Some(ticket.session_token));

        send_and_receive_hello(
            &mut ctrl_send,
            &mut ctrl_recv,
            &mut sm,
            self.node_id(),
            &self.device_name,
            Some(ticket.session_token),
            Some(ticket.node_id),
        )
        .await?;

        let offer_msg: ControlMessage = read_frame(&mut ctrl_recv, MAX_CONTROL_FRAME_SIZE)
            .await?
            .ok_or_else(|| TransportError::Connection("sender closed before Offer".into()))?;

        let manifest = match offer_msg {
            ControlMessage::Offer {
                manifest,
                sender_name,
            } => {
                if manifest.merkle_root != ticket.merkle_root {
                    let _ = write_frame(
                        &mut ctrl_send,
                        &ControlMessage::Reject {
                            reason: "Merkle root mismatch".into(),
                        },
                        MAX_CONTROL_FRAME_SIZE,
                    )
                    .await;
                    return Err(TransportError::MerkleRootMismatch);
                }
                sm.on_receive(ControlMessage::Offer {
                    manifest: manifest.clone(),
                    sender_name,
                })?;
                manifest
            }
            ControlMessage::Cancel { reason } => return Err(TransportError::Cancelled(reason)),
            other => {
                return Err(TransportError::Protocol(
                    core_protocol::ProtocolError::InvalidTransition {
                        state: sm.state().to_string(),
                        event: format!("{other:?}"),
                    },
                ));
            }
        };

        let ct = Arc::new(Mutex::new(initial_connection_type));

        // Watch for live Relayed -> Direct-P2P path upgrades and propagate to telemetry & UI.
        let upgrade_cancel = cancel.child_token();
        let watcher_handle = self.spawn_path_upgrade_watcher(
            transfer_id.clone(),
            ticket.node_id,
            Arc::clone(&ct),
            upgrade_cancel.clone(),
        );
        if initial_connection_type == ConnectionType::Relayed && !upgrade_candidates.is_empty() {
            self.nat.spawn_background_upgrade(
                ticket.node_id,
                upgrade_candidates,
                upgrade_cancel.clone(),
            );
        }

        let summary = run_receiver_session_with_profile(
            transfer_id.clone(),
            conn.clone(),
            ctrl_send,
            ctrl_recv,
            sm,
            manifest,
            output_dir,
            parallel_streams,
            self.profile,
            ct,
            Some(self.progress_tx.clone()),
            cancel,
        )
        .await;

        upgrade_cancel.cancel();
        watcher_handle.abort();
        self.active_cancellations
            .lock()
            .expect("cancellations lock poisoned")
            .remove(&transfer_id);
        graceful_close(&conn).await;
        summary
    }

    /// Pushes files directly to a LAN peer (triggers an incoming approval prompt on the peer).
    pub async fn push_to_peer(
        &self,
        target_node_id: Option<NodeId>,
        target_addr: SocketAddr,
        source: Arc<ChunkSource>,
        connection_type: ConnectionType,
    ) -> Result<TransferSummary> {
        let transfer_id = hex::encode(&source.manifest().session_id[..8]);
        let cancel = self.cancel.child_token();
        self.active_cancellations
            .lock()
            .expect("cancellations lock poisoned")
            .insert(transfer_id.clone(), cancel.clone());

        let conn = self
            .connect_verified_with_timeout(target_addr, target_node_id, Duration::from_secs(15))
            .await?;
        let (mut ctrl_send, mut ctrl_recv) = conn.open_bi().await?;
        let mut sm = HandshakeStateMachine::new(SessionRole::Sender, None);

        send_and_receive_hello(
            &mut ctrl_send,
            &mut ctrl_recv,
            &mut sm,
            self.node_id(),
            &self.device_name,
            None,
            target_node_id,
        )
        .await?;

        let ct = Arc::new(Mutex::new(connection_type));
        let summary = run_sender_session(
            transfer_id.clone(),
            conn.clone(),
            ctrl_send,
            ctrl_recv,
            sm,
            source,
            self.device_name.clone(),
            ct,
            Some(self.progress_tx.clone()),
            cancel,
        )
        .await;

        self.active_cancellations
            .lock()
            .expect("cancellations lock poisoned")
            .remove(&transfer_id);
        graceful_close(&conn).await;
        summary
    }

    /// Pauses a hosted outgoing session (rejecting new pulls and stopping active data streams).
    pub fn pause_hosted_session(&self, transfer_id: &str) -> bool {
        let mut sessions = self
            .hosted_sessions
            .write()
            .expect("hosted_sessions lock poisoned");
        for hosted in sessions.values_mut() {
            if hosted.transfer_id == transfer_id {
                hosted.paused.store(true, Ordering::Release);
                hosted.cancel.cancel();
                // Install a fresh cancellation token so future resume can accept pulls cleanly.
                hosted.cancel = self.cancel.child_token();
                self.active_cancellations
                    .lock()
                    .expect("cancellations lock poisoned")
                    .insert(transfer_id.to_string(), hosted.cancel.clone());
                return true;
            }
        }
        false
    }

    /// Resumes a previously paused hosted outgoing session.
    pub fn resume_hosted_session(&self, transfer_id: &str) -> bool {
        let sessions = self
            .hosted_sessions
            .read()
            .expect("hosted_sessions lock poisoned");
        for hosted in sessions.values() {
            if hosted.transfer_id == transfer_id {
                hosted.paused.store(false, Ordering::Release);
                return true;
            }
        }
        false
    }

    /// Cancels an active outgoing or incoming transfer by ID.
    pub fn cancel_transfer(&self, transfer_id: &str) -> bool {
        if let Some(token) = self
            .active_cancellations
            .lock()
            .expect("cancellations lock poisoned")
            .remove(transfer_id)
        {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Stops accepting new connections and shuts down discovery & NAT tasks.
    pub fn shutdown(&self) {
        self.cancel.cancel();
        if let Some(ref disc) = self.discovery {
            disc.shutdown();
        }
        self.endpoint.close(0u32.into(), b"shutdown");
    }

    fn spawn_path_upgrade_watcher(
        &self,
        transfer_id: String,
        peer_node_id: NodeId,
        ct: Arc<Mutex<ConnectionType>>,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let mut rx = self.magic_socket.subscribe_path_changes();
        let tx = self.conn_changed_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    evt = rx.recv() => {
                        match evt {
                            Ok(change) if change.peer_node_id == peer_node_id => {
                                *ct.lock().expect("conn_type lock poisoned") = change.connection_type;
                                let _ = tx.send(TransferConnectionChanged {
                                    transfer_id: transfer_id.clone(),
                                    peer_node_id,
                                    connection_type: change.connection_type,
                                });
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                            _ => {}
                        }
                    }
                }
            }
        })
    }

    async fn connect_verified_with_timeout(
        &self,
        addr: SocketAddr,
        expected_peer: Option<NodeId>,
        timeout: Duration,
    ) -> Result<quinn::Connection> {
        let client_cfg = self
            .identity
            .build_client_config(expected_peer, Arc::clone(&self.transport_config))?;
        let server_name = expected_peer
            .map(|id| format!("{}.veloce.local", id.to_base32()))
            .unwrap_or_else(|| "veloce.local".into());
        let connecting = self
            .endpoint
            .connect_with(client_cfg, addr, &server_name)?;
        let conn = tokio::time::timeout(timeout, connecting)
            .await
            .map_err(|_| TransportError::Timeout(format!("QUIC connect to {addr}")))??;
        Ok(conn)
    }

    fn spawn_accept_loop(
        self: &Arc<Self>,
        offers_tx: mpsc::UnboundedSender<IncomingOffer>,
        parallel_streams: usize,
    ) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = this.cancel.cancelled() => break,
                    incoming = this.endpoint.accept() => {
                        let Some(connecting) = incoming else { break };
                        let node = Arc::clone(&this);
                        let offers = offers_tx.clone();
                        tokio::spawn(async move {
                            let conn = match connecting.await {
                                Ok(c) => c,
                                Err(err) => {
                                    tracing::debug!("Incoming QUIC handshake failed: {}", err);
                                    return;
                                }
                            };
                            if let Err(err) = node.handle_incoming_connection(conn, offers, parallel_streams).await {
                                tracing::debug!("Incoming session ended: {}", err);
                            }
                        });
                    }
                }
            }
        });
    }

    async fn handle_incoming_connection(
        &self,
        conn: quinn::Connection,
        offers_tx: mpsc::UnboundedSender<IncomingOffer>,
        parallel_streams: usize,
    ) -> Result<()> {
        let authed_peer = peer_node_id(&conn)?;
        let (mut ctrl_send, mut ctrl_recv) = conn.accept_bi().await?;

        let first_msg: ControlMessage = read_frame(&mut ctrl_recv, MAX_CONTROL_FRAME_SIZE)
            .await?
            .ok_or_else(|| TransportError::Connection("empty control stream".into()))?;

        let (claimed_node_id, peer_name, session_token) = match &first_msg {
            ControlMessage::Hello {
                node_id,
                device_name,
                session_token,
                ..
            } => (*node_id, device_name.clone(), *session_token),
            _ => {
                return Err(TransportError::Protocol(
                    core_protocol::ProtocolError::InvalidTransition {
                        state: "Init".into(),
                        event: format!("{first_msg:?}"),
                    },
                ));
            }
        };

        if claimed_node_id != authed_peer {
            return Err(TransportError::PeerMismatch {
                expected: authed_peer.to_base32(),
                actual: claimed_node_id.to_base32(),
            });
        }

        let remote_addr = conn.remote_address();
        let conn_type = self
            .magic_socket
            .connection_type_for_addr(remote_addr)
            .unwrap_or_else(|| classify_socket_addr(remote_addr));

        if let Some(token) = session_token {
            let hosted = self
                .hosted_sessions
                .read()
                .expect("hosted_sessions lock poisoned")
                .get(&token)
                .cloned()
                .ok_or(TransportError::SessionNotFound)?;

            if hosted.paused.load(Ordering::Acquire) {
                let _ = write_frame(
                    &mut ctrl_send,
                    &ControlMessage::Cancel {
                        reason: "session paused by sender".into(),
                    },
                    MAX_CONTROL_FRAME_SIZE,
                )
                .await;
                let _ = ctrl_send.finish();
                let _ = tokio::time::timeout(Duration::from_millis(100), conn.closed()).await;
                return Err(TransportError::Cancelled("session paused by sender".into()));
            }

            let mut sm = HandshakeStateMachine::new(SessionRole::Sender, Some(token));
            sm.on_receive(first_msg)?;
            let reply_hello = ControlMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                node_id: self.node_id(),
                device_name: self.device_name.clone(),
                session_token: Some(token),
            };
            sm.on_send(&reply_hello)?;
            write_frame(&mut ctrl_send, &reply_hello, MAX_CONTROL_FRAME_SIZE).await?;

            let ct = Arc::new(Mutex::new(conn_type));
            let watch_cancel = hosted.cancel.child_token();
            let watcher = self.spawn_path_upgrade_watcher(
                hosted.transfer_id.clone(),
                authed_peer,
                Arc::clone(&ct),
                watch_cancel.clone(),
            );
            let res = run_sender_session(
                hosted.transfer_id.clone(),
                conn.clone(),
                ctrl_send,
                ctrl_recv,
                sm,
                hosted.source,
                self.device_name.clone(),
                ct,
                Some(self.progress_tx.clone()),
                hosted.cancel,
            )
            .await;
            watch_cancel.cancel();
            watcher.abort();
            res?;
        } else {
            let mut sm = HandshakeStateMachine::new(SessionRole::Receiver, None);
            sm.on_receive(first_msg)?;
            let reply_hello = ControlMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                node_id: self.node_id(),
                device_name: self.device_name.clone(),
                session_token: None,
            };
            sm.on_send(&reply_hello)?;
            write_frame(&mut ctrl_send, &reply_hello, MAX_CONTROL_FRAME_SIZE).await?;

            let offer_msg: ControlMessage = read_frame(&mut ctrl_recv, MAX_CONTROL_FRAME_SIZE)
                .await?
                .ok_or_else(|| TransportError::Connection("closed before Offer".into()))?;

            let (manifest, sender_name) = match offer_msg {
                ControlMessage::Offer {
                    manifest,
                    sender_name,
                } => {
                    sm.on_receive(ControlMessage::Offer {
                        manifest: manifest.clone(),
                        sender_name: sender_name.clone(),
                    })?;
                    (manifest, sender_name)
                }
                ControlMessage::Cancel { reason } => return Err(TransportError::Cancelled(reason)),
                other => {
                    return Err(TransportError::Protocol(
                        core_protocol::ProtocolError::InvalidTransition {
                            state: sm.state().to_string(),
                            event: format!("{other:?}"),
                        },
                    ));
                }
            };

            let transfer_id = hex::encode(&manifest.session_id[..8]);
            let (resp_tx, resp_rx) = oneshot::channel();
            let offer = IncomingOffer {
                transfer_id: transfer_id.clone(),
                sender_node_id: authed_peer,
                sender_name: if sender_name.is_empty() {
                    peer_name
                } else {
                    sender_name
                },
                manifest: manifest.clone(),
                connection_type: conn_type,
                responder: resp_tx,
            };

            if offers_tx.send(offer).is_err() {
                let _ = write_frame(
                    &mut ctrl_send,
                    &ControlMessage::Reject {
                        reason: "receiver not accepting transfers".into(),
                    },
                    MAX_CONTROL_FRAME_SIZE,
                )
                .await;
                let _ = ctrl_send.finish();
                let _ = tokio::time::timeout(Duration::from_millis(200), conn.closed()).await;
                return Err(TransportError::Rejected("no offer handler".into()));
            }

            let decision = match resp_rx.await {
                Ok(d) => d,
                Err(_) => IncomingOfferDecision::Reject {
                    reason: "offer dismissed".into(),
                },
            };

            match decision {
                IncomingOfferDecision::Accept { output_dir } => {
                    let cancel = self.cancel.child_token();
                    self.active_cancellations
                        .lock()
                        .expect("cancellations lock poisoned")
                        .insert(transfer_id.clone(), cancel.clone());
                    let ct = Arc::new(Mutex::new(conn_type));
                    let res = run_receiver_session_with_profile(
                        transfer_id.clone(),
                        conn.clone(),
                        ctrl_send,
                        ctrl_recv,
                        sm,
                        manifest,
                        output_dir,
                        parallel_streams,
                        self.profile,
                        ct,
                        Some(self.progress_tx.clone()),
                        cancel,
                    )
                    .await;
                    self.active_cancellations
                        .lock()
                        .expect("cancellations lock poisoned")
                        .remove(&transfer_id);
                    res?;
                }
                IncomingOfferDecision::Reject { reason } => {
                    let reject = ControlMessage::Reject {
                        reason: reason.clone(),
                    };
                    let _ = sm.on_send(&reject);
                    let _ = write_frame(&mut ctrl_send, &reject, MAX_CONTROL_FRAME_SIZE).await;
                    let _ = ctrl_send.finish();
                    let _ = tokio::time::timeout(Duration::from_millis(200), conn.closed()).await;
                    return Err(TransportError::Rejected(reason));
                }
            }
        }

        Ok(())
    }
}

/// Classifies a direct socket address as `Lan` (private/loopback/link-local) or `DirectP2p` (public WAN).
pub fn classify_socket_addr(addr: SocketAddr) -> ConnectionType {
    match addr.ip() {
        IpAddr::V4(v4) => {
            if v4.is_private() || v4.is_loopback() || v4.is_link_local() {
                ConnectionType::Lan
            } else {
                ConnectionType::DirectP2p
            }
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || ((v6.segments()[0] & 0xfe00) == 0xfc00) {
                ConnectionType::Lan
            } else {
                ConnectionType::DirectP2p
            }
        }
    }
}
