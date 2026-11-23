use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use core_protocol::{ConnectionType, NodeId, SessionTicket};
use core_storage::ChunkSource;
use core_transport::{
    DiscoveryEvent, EndpointOptions, IncomingOfferDecision, LocalPeer, TransferDirection,
    VeloceEndpoint, DEFAULT_PARALLEL_STREAMS,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{broadcast, oneshot};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Error)]
pub enum FfiError {
    #[error("engine not initialized")]
    NotInitialized,

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("protocol error: {0}")]
    Protocol(#[from] core_protocol::ProtocolError),

    #[error("storage error: {0}")]
    Storage(#[from] core_storage::StorageError),

    #[error("transport error: {0}")]
    Transport(#[from] core_transport::TransportError),

    #[error("transfer offer '{0}' not found or already resolved")]
    OfferNotFound(String),
}

pub type Result<T> = std::result::Result<T, FfiError>;

/// Engine initialization configuration passed across the FFI boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConfigDto {
    pub device_name: String,
    pub bind_addr: Option<String>,
    pub enable_mdns: Option<bool>,
    pub parallel_streams: Option<u32>,
    pub relay_addrs: Option<Vec<String>>,
    pub force_relay: Option<bool>,
}

/// Static information about the local Veloce node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfoDto {
    pub node_id: String,
    pub short_id: String,
    pub device_name: String,
    pub listen_addr: String,
    pub listen_port: u16,
}

/// Serializable descriptor of a nearby LAN peer discovered via mDNS.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerDto {
    pub node_id: String,
    pub short_id: String,
    pub device_name: String,
    pub addresses: Vec<String>,
    pub port: u16,
}

impl From<LocalPeer> for PeerDto {
    fn from(peer: LocalPeer) -> Self {
        Self {
            node_id: peer.node_id.to_base32(),
            short_id: peer.node_id.short_id(),
            device_name: peer.device_name,
            addresses: peer.addresses.into_iter().map(|a| a.to_string()).collect(),
            port: peer.port,
        }
    }
}

/// File entry summary exposed to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSummaryDto {
    pub relative_path: String,
    pub size: u64,
}

/// Response returned immediately when starting a send session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSessionResultDto {
    pub transfer_id: String,
    pub ticket_code: String,
    pub qr_uri: String,
    pub total_size: u64,
    pub total_chunks: u32,
    pub chunk_size: u32,
    pub file_count: u32,
    pub files: Vec<FileSummaryDto>,
}

/// Decoded preview of a Global Session Ticket before initiating download.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketPreviewDto {
    pub transfer_id: String,
    pub sender_node_id: String,
    pub sender_short_id: String,
    pub total_size: u64,
    pub file_count: u32,
    pub direct_addrs: Vec<String>,
    pub relay_hints: Vec<String>,
    pub merkle_root_hex: String,
}

/// Response returned when initiating a receive/join session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinSessionResultDto {
    pub transfer_id: String,
    pub total_size: u64,
    pub file_count: u32,
}

/// Tagged union of all asynchronous events streamed from Rust to the decoupled frontend UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EngineEvent {
    #[serde(rename_all = "camelCase")]
    TransferProgress {
        transfer_id: String,
        direction: String,
        bytes_transferred: u64,
        total_bytes: u64,
        chunks_verified: u32,
        total_chunks: u32,
        speed_bytes_per_sec: f64,
        eta_seconds: f64,
        connection_type: String,
    },
    #[serde(rename_all = "camelCase")]
    PeerDiscovered {
        peer: PeerDto,
    },
    #[serde(rename_all = "camelCase")]
    PeerLost {
        node_id: String,
    },
    #[serde(rename_all = "camelCase")]
    IncomingOffer {
        transfer_id: String,
        sender_node_id: String,
        sender_short_id: String,
        sender_name: String,
        file_count: u32,
        total_size: u64,
        files: Vec<FileSummaryDto>,
        connection_type: String,
    },
    #[serde(rename_all = "camelCase")]
    ConnectionTypeChanged {
        transfer_id: String,
        peer_node_id: String,
        connection_type: String,
    },
    #[serde(rename_all = "camelCase")]
    TransferPaused {
        transfer_id: String,
        direction: String,
    },
    #[serde(rename_all = "camelCase")]
    TransferResumed {
        transfer_id: String,
        direction: String,
    },
    #[serde(rename_all = "camelCase")]
    TransferCompleted {
        transfer_id: String,
        direction: String,
        total_bytes: u64,
        total_chunks: u32,
        elapsed_ms: u64,
        average_speed_bytes_per_sec: f64,
        output_paths: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    TransferFailed {
        transfer_id: String,
        direction: String,
        error: String,
    },
}

#[derive(Debug, Clone)]
struct ManagedTransfer {
    direction: String,
    ticket_code: Option<String>,
    output_dir: Option<PathBuf>,
    parallel_streams: usize,
    hosted: bool,
    paused: bool,
}

/// Core FFI engine orchestrating `VeloceEndpoint`, pending incoming LAN offers,
/// resumable pause/resume state, non-blocking background transfer tasks, and a unified event stream.
pub struct VeloceEngine {
    endpoint: Arc<VeloceEndpoint>,
    pending_offers: Arc<Mutex<HashMap<String, oneshot::Sender<IncomingOfferDecision>>>>,
    managed_transfers: Arc<Mutex<HashMap<String, ManagedTransfer>>>,
    events_tx: broadcast::Sender<EngineEvent>,
    parallel_streams: usize,
    cancel: CancellationToken,
}

impl VeloceEngine {
    /// Initializes the underlying QUIC BBR endpoint, mDNS discovery, NAT/relay coordinator,
    /// and background event aggregation tasks.
    pub async fn init(config: EngineConfigDto) -> Result<Arc<Self>> {
        let bind_addr = match config.bind_addr {
            Some(ref s) if !s.trim().is_empty() => s
                .parse::<SocketAddr>()
                .map_err(|e| FfiError::InvalidArgument(format!("invalid bind_addr '{s}': {e}")))?,
            _ => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        };

        let mut relay_addrs = Vec::new();
        if let Some(relays) = config.relay_addrs {
            for r in relays {
                let trimmed = r.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(resolved) = tokio::net::lookup_host(trimmed).await {
                    for addr in resolved {
                        if !relay_addrs.contains(&addr) {
                            relay_addrs.push(addr);
                        }
                    }
                }
            }
        }

        let parallel_streams = config
            .parallel_streams
            .map(|n| (n as usize).clamp(1, 32))
            .unwrap_or(DEFAULT_PARALLEL_STREAMS);

        let opts = EndpointOptions {
            bind_addr,
            device_name: if config.device_name.trim().is_empty() {
                "Veloce Device".into()
            } else {
                config.device_name
            },
            enable_mdns: config.enable_mdns.unwrap_or(true),
            parallel_streams,
            relay_addrs,
            force_relay: config.force_relay.unwrap_or(false),
        };

        let (endpoint, mut channels) = VeloceEndpoint::bind(opts).await?;
        let (events_tx, _) = broadcast::channel(512);
        let pending_offers: Arc<Mutex<HashMap<String, oneshot::Sender<IncomingOfferDecision>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let cancel = CancellationToken::new();

        // Forward real-time transfer progress snapshots.
        let tx_prog = events_tx.clone();
        let cancel_prog = cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_prog.cancelled() => break,
                    item = channels.progress_rx.recv() => {
                        let Some(p) = item else { break };
                        let _ = tx_prog.send(EngineEvent::TransferProgress {
                            transfer_id: p.transfer_id,
                            direction: direction_str(p.direction).into(),
                            bytes_transferred: p.bytes_transferred,
                            total_bytes: p.total_bytes,
                            chunks_verified: p.chunks_verified,
                            total_chunks: p.total_chunks,
                            speed_bytes_per_sec: p.speed_bytes_per_sec,
                            eta_seconds: p.eta_seconds,
                            connection_type: p.connection_type.as_str().into(),
                        });
                    }
                }
            }
        });

        // Forward incoming LAN transfer offers and store their approval channels.
        let tx_offers = events_tx.clone();
        let pending_bg = Arc::clone(&pending_offers);
        let cancel_offers = cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_offers.cancelled() => break,
                    item = channels.incoming_offers.recv() => {
                        let Some(offer) = item else { break };
                        let files = offer
                            .manifest
                            .files
                            .iter()
                            .map(|f| FileSummaryDto {
                                relative_path: f.relative_path.clone(),
                                size: f.size,
                            })
                            .collect();
                        let tid = offer.transfer_id.clone();
                        pending_bg
                            .lock()
                            .expect("pending_offers lock poisoned")
                            .insert(tid.clone(), offer.responder);
                        let _ = tx_offers.send(EngineEvent::IncomingOffer {
                            transfer_id: tid,
                            sender_node_id: offer.sender_node_id.to_base32(),
                            sender_short_id: offer.sender_node_id.short_id(),
                            sender_name: offer.sender_name,
                            file_count: offer.manifest.files.len() as u32,
                            total_size: offer.manifest.total_size,
                            files,
                            connection_type: offer.connection_type.as_str().into(),
                        });
                    }
                }
            }
        });

        // Forward ConnectionTypeChanged events (LAN / Direct-P2P / Relayed).
        let tx_conn = events_tx.clone();
        let cancel_conn = cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_conn.cancelled() => break,
                    evt = channels.conn_changed_rx.recv() => {
                        match evt {
                            Ok(c) => {
                                let _ = tx_conn.send(EngineEvent::ConnectionTypeChanged {
                                    transfer_id: c.transfer_id,
                                    peer_node_id: c.peer_node_id.to_base32(),
                                    connection_type: c.connection_type.as_str().into(),
                                });
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                            Err(broadcast::error::RecvError::Lagged(_)) => {}
                        }
                    }
                }
            }
        });

        // Forward mDNS peer discovery events.
        if let Some(mut disc_rx) = endpoint.subscribe_discovery() {
            let tx_disc = events_tx.clone();
            let cancel_disc = cancel.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = cancel_disc.cancelled() => break,
                        evt = disc_rx.recv() => {
                            match evt {
                                Ok(DiscoveryEvent::PeerDiscovered(peer)) => {
                                    let _ = tx_disc.send(EngineEvent::PeerDiscovered {
                                        peer: PeerDto::from(peer),
                                    });
                                }
                                Ok(DiscoveryEvent::PeerLost(node_id)) => {
                                    let _ = tx_disc.send(EngineEvent::PeerLost {
                                        node_id: node_id.to_base32(),
                                    });
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                                Err(broadcast::error::RecvError::Lagged(_)) => {}
                            }
                        }
                    }
                }
            });
        }

        Ok(Arc::new(Self {
            endpoint,
            pending_offers,
            managed_transfers: Arc::new(Mutex::new(HashMap::new())),
            events_tx,
            parallel_streams,
            cancel,
        }))
    }

    pub fn node_info(&self) -> NodeInfoDto {
        let addr = self.endpoint.listen_addr();
        let id = self.endpoint.node_id();
        NodeInfoDto {
            node_id: id.to_base32(),
            short_id: id.short_id(),
            device_name: self.endpoint.device_name().to_string(),
            listen_addr: addr.to_string(),
            listen_port: addr.port(),
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<EngineEvent> {
        self.events_tx.subscribe()
    }

    pub fn list_local_peers(&self) -> Vec<PeerDto> {
        self.endpoint
            .local_peers()
            .into_iter()
            .map(PeerDto::from)
            .collect()
    }

    /// Parses and validates a `veloce1...` ticket string for UI preview without connecting.
    pub fn parse_ticket(ticket_code: &str) -> Result<TicketPreviewDto> {
        let ticket = SessionTicket::decode(ticket_code)?;
        Ok(TicketPreviewDto {
            transfer_id: hex::encode(&ticket.merkle_root[..8]),
            sender_node_id: ticket.node_id.to_base32(),
            sender_short_id: ticket.node_id.short_id(),
            total_size: ticket.total_size,
            file_count: ticket.file_count,
            direct_addrs: ticket
                .direct_addrs
                .into_iter()
                .map(|a| a.to_string())
                .collect(),
            relay_hints: ticket.relay_hints,
            merkle_root_hex: hex::encode(ticket.merkle_root),
        })
    }

    /// Indexes files/folders with BLAKE3 Merkle chunking, hosts a global ticket session,
    /// and optionally initiates an immediate LAN push if `target_peer_node_id` is supplied.
    pub async fn start_session(
        &self,
        paths: Vec<String>,
        target_peer_node_id: Option<String>,
        relay_hints: Vec<String>,
    ) -> Result<StartSessionResultDto> {
        if paths.is_empty() {
            return Err(FfiError::InvalidArgument(
                "at least one file or directory path is required".into(),
            ));
        }

        let path_bufs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let source = ChunkSource::from_paths_async(path_bufs, None).await?;
        let manifest = source.manifest();

        let hosted = self.endpoint.host_session(
            Arc::clone(&source),
            vec![self.endpoint.listen_addr()],
            relay_hints,
        )?;

        let result = StartSessionResultDto {
            transfer_id: hosted.transfer_id.clone(),
            ticket_code: hosted.ticket_code.clone(),
            qr_uri: format!("veloce://{}", hosted.ticket_code),
            total_size: manifest.total_size,
            total_chunks: manifest.total_chunks,
            chunk_size: manifest.chunk_size,
            file_count: manifest.files.len() as u32,
            files: manifest
                .files
                .iter()
                .map(|f| FileSummaryDto {
                    relative_path: f.relative_path.clone(),
                    size: f.size,
                })
                .collect(),
        };

        self.managed_transfers
            .lock()
            .expect("managed_transfers lock poisoned")
            .insert(
                result.transfer_id.clone(),
                ManagedTransfer {
                    direction: "send".into(),
                    ticket_code: Some(result.ticket_code.clone()),
                    output_dir: None,
                    parallel_streams: self.parallel_streams,
                    hosted: true,
                    paused: false,
                },
            );

        // If caller picked a specific LAN peer, push directly in a non-blocking background task.
        if let Some(peer_id_str) = target_peer_node_id {
            let trimmed = peer_id_str.trim();
            if !trimmed.is_empty() {
                let target_id = NodeId::parse(trimmed)?;
                let target_peer = self
                    .endpoint
                    .local_peers()
                    .into_iter()
                    .find(|p| p.node_id == target_id)
                    .ok_or_else(|| {
                        FfiError::InvalidArgument(format!("LAN peer '{trimmed}' not discovered"))
                    })?;
                let target_addr = *target_peer.addresses.first().ok_or_else(|| {
                    FfiError::InvalidArgument("LAN peer has no reachable address".into())
                })?;

                let ep = Arc::clone(&self.endpoint);
                let tx = self.events_tx.clone();
                let tid = result.transfer_id.clone();
                let src = Arc::clone(&source);

                tokio::spawn(async move {
                    let _ = tx.send(EngineEvent::ConnectionTypeChanged {
                        transfer_id: tid.clone(),
                        peer_node_id: target_id.to_base32(),
                        connection_type: ConnectionType::Lan.as_str().into(),
                    });
                    match ep
                        .push_to_peer(Some(target_id), target_addr, src, ConnectionType::Lan)
                        .await
                    {
                        Ok(summary) => {
                            let _ = tx.send(EngineEvent::TransferCompleted {
                                transfer_id: tid,
                                direction: "send".into(),
                                total_bytes: summary.total_bytes,
                                total_chunks: summary.total_chunks,
                                elapsed_ms: summary.elapsed_ms,
                                average_speed_bytes_per_sec: summary.average_speed_bytes_per_sec,
                                output_paths: Vec::new(),
                            });
                        }
                        Err(err) => {
                            let _ = tx.send(EngineEvent::TransferFailed {
                                transfer_id: tid,
                                direction: "send".into(),
                                error: err.to_string(),
                            });
                        }
                    }
                });
            }
        }

        Ok(result)
    }

    /// Joins a global session using a `veloce1...` ticket string and downloads all chunks
    /// in the background while streaming live progress & connection-type events.
    pub async fn join_session(
        &self,
        ticket_code: String,
        output_dir: String,
        parallel_streams: Option<u32>,
    ) -> Result<JoinSessionResultDto> {
        let ticket = SessionTicket::decode(&ticket_code)?;
        let out_dir = PathBuf::from(output_dir.trim());
        if out_dir.as_os_str().is_empty() {
            return Err(FfiError::InvalidArgument(
                "output_dir must not be empty".into(),
            ));
        }

        let transfer_id = hex::encode(&ticket.merkle_root[..8]);
        let total_size = ticket.total_size;
        let file_count = ticket.file_count;
        let streams = parallel_streams
            .map(|n| (n as usize).clamp(1, 32))
            .unwrap_or(self.parallel_streams);

        self.managed_transfers
            .lock()
            .expect("managed_transfers lock poisoned")
            .insert(
                transfer_id.clone(),
                ManagedTransfer {
                    direction: "receive".into(),
                    ticket_code: Some(ticket_code),
                    output_dir: Some(out_dir.clone()),
                    parallel_streams: streams,
                    hosted: false,
                    paused: false,
                },
            );

        self.spawn_join_task(transfer_id.clone(), ticket, out_dir, streams);

        Ok(JoinSessionResultDto {
            transfer_id,
            total_size,
            file_count,
        })
    }

    fn spawn_join_task(
        &self,
        transfer_id: String,
        ticket: SessionTicket,
        out_dir: PathBuf,
        streams: usize,
    ) {
        let ep = Arc::clone(&self.endpoint);
        let tx = self.events_tx.clone();
        let managed = Arc::clone(&self.managed_transfers);
        let tid = transfer_id;

        tokio::spawn(async move {
            match ep.join_session(&ticket, out_dir, Some(streams)).await {
                Ok(summary) => {
                    managed
                        .lock()
                        .expect("managed_transfers lock poisoned")
                        .remove(&tid);
                    let _ = tx.send(EngineEvent::TransferCompleted {
                        transfer_id: tid,
                        direction: "receive".into(),
                        total_bytes: summary.total_bytes,
                        total_chunks: summary.total_chunks,
                        elapsed_ms: summary.elapsed_ms,
                        average_speed_bytes_per_sec: summary.average_speed_bytes_per_sec,
                        output_paths: summary
                            .finalized_paths
                            .into_iter()
                            .map(|p| p.display().to_string())
                            .collect(),
                    });
                }
                Err(err) => {
                    // If the transfer was explicitly paused, suppress TransferFailed so the UI
                    // keeps the paused state & .veloce-resume-*.state intact for later resume.
                    let is_paused = managed
                        .lock()
                        .expect("managed_transfers lock poisoned")
                        .get(&tid)
                        .is_some_and(|m| m.paused);
                    if !is_paused {
                        managed
                            .lock()
                            .expect("managed_transfers lock poisoned")
                            .remove(&tid);
                        let _ = tx.send(EngineEvent::TransferFailed {
                            transfer_id: tid,
                            direction: "receive".into(),
                            error: err.to_string(),
                        });
                    }
                }
            }
        });
    }

    /// Pauses an active transfer while preserving verified chunks in `.veloce-resume-*.state`
    /// so `resume_transfer` continues from the exact missing chunks without re-transferring data.
    pub fn pause_transfer(&self, transfer_id: &str) -> bool {
        let (direction, hosted) = {
            let mut map = self
                .managed_transfers
                .lock()
                .expect("managed_transfers lock poisoned");
            let Some(entry) = map.get_mut(transfer_id) else {
                return false;
            };
            if entry.paused {
                return false;
            }
            entry.paused = true;
            (entry.direction.clone(), entry.hosted)
        };

        if hosted {
            self.endpoint.pause_hosted_session(transfer_id);
        } else {
            self.endpoint.cancel_transfer(transfer_id);
        }

        let _ = self.events_tx.send(EngineEvent::TransferPaused {
            transfer_id: transfer_id.to_string(),
            direction,
        });
        true
    }

    /// Resumes a previously paused transfer using its saved `.veloce-resume-*.state` bitfield.
    pub async fn resume_transfer(&self, transfer_id: &str) -> Result<bool> {
        let resume_plan = {
            let mut map = self
                .managed_transfers
                .lock()
                .expect("managed_transfers lock poisoned");
            let Some(entry) = map.get_mut(transfer_id) else {
                return Ok(false);
            };
            if !entry.paused {
                return Ok(false);
            }
            entry.paused = false;
            entry.clone()
        };

        let _ = self.events_tx.send(EngineEvent::TransferResumed {
            transfer_id: transfer_id.to_string(),
            direction: resume_plan.direction.clone(),
        });

        if resume_plan.hosted {
            self.endpoint.resume_hosted_session(transfer_id);
            return Ok(true);
        }

        if let (Some(ticket_code), Some(out_dir)) = (resume_plan.ticket_code, resume_plan.output_dir)
        {
            let ticket = SessionTicket::decode(&ticket_code)?;
            self.spawn_join_task(
                transfer_id.to_string(),
                ticket,
                out_dir,
                resume_plan.parallel_streams,
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// Approves a pending incoming LAN transfer offer and starts writing verified chunks to `output_dir`.
    pub fn accept_incoming_transfer(&self, transfer_id: &str, output_dir: String) -> Result<()> {
        let responder = self
            .pending_offers
            .lock()
            .expect("pending_offers lock poisoned")
            .remove(transfer_id)
            .ok_or_else(|| FfiError::OfferNotFound(transfer_id.to_string()))?;
        let _ = responder.send(IncomingOfferDecision::Accept {
            output_dir: PathBuf::from(output_dir),
        });
        Ok(())
    }

    /// Rejects a pending incoming LAN transfer offer.
    pub fn reject_incoming_transfer(&self, transfer_id: &str, reason: Option<String>) -> Result<()> {
        let responder = self
            .pending_offers
            .lock()
            .expect("pending_offers lock poisoned")
            .remove(transfer_id)
            .ok_or_else(|| FfiError::OfferNotFound(transfer_id.to_string()))?;
        let _ = responder.send(IncomingOfferDecision::Reject {
            reason: reason.unwrap_or_else(|| "declined by user".into()),
        });
        Ok(())
    }

    /// Cancels an active or paused transfer by ID.
    pub fn cancel_transfer(&self, transfer_id: &str) -> bool {
        let was_managed = self
            .managed_transfers
            .lock()
            .expect("managed_transfers lock poisoned")
            .remove(transfer_id)
            .is_some();
        if let Some(responder) = self
            .pending_offers
            .lock()
            .expect("pending_offers lock poisoned")
            .remove(transfer_id)
        {
            let _ = responder.send(IncomingOfferDecision::Reject {
                reason: "cancelled by user".into(),
            });
            return true;
        }
        self.endpoint.cancel_transfer(transfer_id) || was_managed
    }

    /// Cleanly shuts down the endpoint and all background tasks.
    pub fn shutdown(&self) {
        self.cancel.cancel();
        self.endpoint.shutdown();
    }
}

fn direction_str(d: TransferDirection) -> &'static str {
    match d {
        TransferDirection::Send => "send",
        TransferDirection::Receive => "receive",
    }
}
