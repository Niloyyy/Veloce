use std::sync::{Arc, RwLock};

use core_ffi::{
    EngineConfigDto, EngineEvent, FileSummaryDto, JoinSessionResultDto, NodeInfoDto, PeerDto,
    StartSessionResultDto, TicketPreviewDto, VeloceEngine,
};
use core_protocol::TransferProfile;
use flutter_rust_bridge::frb;
use tokio::sync::broadcast;

use crate::frb_generated::StreamSink;

static MOBILE_ENGINE: RwLock<Option<MobileState>> = RwLock::new(None);

#[derive(Clone)]
struct MobileState {
    engine: Arc<VeloceEngine>,
    default_download_dir: String,
}

/// Mobile-exposed transfer tuning profile (`Desktop` vs `Mobile` bounded-memory mode).
#[frb]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobileTransferProfile {
    Desktop,
    Mobile,
}

impl From<MobileTransferProfile> for TransferProfile {
    fn from(value: MobileTransferProfile) -> Self {
        match value {
            MobileTransferProfile::Desktop => TransferProfile::Desktop,
            MobileTransferProfile::Mobile => TransferProfile::Mobile,
        }
    }
}

/// Local Veloce node metadata returned to Flutter upon initialization.
#[frb]
#[derive(Debug, Clone)]
pub struct MobileNodeInfo {
    pub node_id: String,
    pub short_id: String,
    pub device_name: String,
    pub listen_addr: String,
    pub listen_port: u16,
    pub default_download_dir: String,
}

impl MobileNodeInfo {
    fn from_dto(dto: NodeInfoDto, default_download_dir: String) -> Self {
        Self {
            node_id: dto.node_id,
            short_id: dto.short_id,
            device_name: dto.device_name,
            listen_addr: dto.listen_addr,
            listen_port: dto.listen_port,
            default_download_dir,
        }
    }
}

/// Nearby mDNS LAN peer descriptor exposed to Flutter.
#[frb]
#[derive(Debug, Clone)]
pub struct MobilePeer {
    pub node_id: String,
    pub short_id: String,
    pub device_name: String,
    pub addresses: Vec<String>,
    pub port: u16,
}

impl From<PeerDto> for MobilePeer {
    fn from(p: PeerDto) -> Self {
        Self {
            node_id: p.node_id,
            short_id: p.short_id,
            device_name: p.device_name,
            addresses: p.addresses,
            port: p.port,
        }
    }
}

/// File entry preview inside an offer or hosted session.
#[frb]
#[derive(Debug, Clone)]
pub struct MobileFileSummary {
    pub relative_path: String,
    pub size: u64,
}

impl From<FileSummaryDto> for MobileFileSummary {
    fn from(f: FileSummaryDto) -> Self {
        Self {
            relative_path: f.relative_path,
            size: f.size,
        }
    }
}

/// Result returned when hosting a send session (includes Global Ticket + QR URI).
#[frb]
#[derive(Debug, Clone)]
pub struct MobileStartSessionResult {
    pub transfer_id: String,
    pub ticket_code: String,
    pub qr_uri: String,
    pub total_size: u64,
    pub total_chunks: u32,
    pub chunk_size: u32,
    pub file_count: u32,
    pub files: Vec<MobileFileSummary>,
}

impl From<StartSessionResultDto> for MobileStartSessionResult {
    fn from(r: StartSessionResultDto) -> Self {
        Self {
            transfer_id: r.transfer_id,
            ticket_code: r.ticket_code,
            qr_uri: r.qr_uri,
            total_size: r.total_size,
            total_chunks: r.total_chunks,
            chunk_size: r.chunk_size,
            file_count: r.file_count,
            files: r.files.into_iter().map(MobileFileSummary::from).collect(),
        }
    }
}

/// Decoded ticket preview rendered immediately after scanning a QR code or pasting a ticket.
#[frb]
#[derive(Debug, Clone)]
pub struct MobileTicketPreview {
    pub transfer_id: String,
    pub sender_node_id: String,
    pub sender_short_id: String,
    pub total_size: u64,
    pub file_count: u32,
    pub direct_addrs: Vec<String>,
    pub relay_hints: Vec<String>,
    pub merkle_root_hex: String,
}

impl From<TicketPreviewDto> for MobileTicketPreview {
    fn from(t: TicketPreviewDto) -> Self {
        Self {
            transfer_id: t.transfer_id,
            sender_node_id: t.sender_node_id,
            sender_short_id: t.sender_short_id,
            total_size: t.total_size,
            file_count: t.file_count,
            direct_addrs: t.direct_addrs,
            relay_hints: t.relay_hints,
            merkle_root_hex: t.merkle_root_hex,
        }
    }
}

/// Response returned when joining a remote ticket session.
#[frb]
#[derive(Debug, Clone)]
pub struct MobileJoinSessionResult {
    pub transfer_id: String,
    pub total_size: u64,
    pub file_count: u32,
}

impl From<JoinSessionResultDto> for MobileJoinSessionResult {
    fn from(j: JoinSessionResultDto) -> Self {
        Self {
            transfer_id: j.transfer_id,
            total_size: j.total_size,
            file_count: j.file_count,
        }
    }
}

/// Typed real-time event stream pushed from Rust to Flutter via `StreamSink<MobileEngineEvent>`.
#[frb]
#[derive(Debug, Clone)]
pub enum MobileEngineEvent {
    TransferProgress {
        transfer_id: String,
        direction: String,
        bytes_transferred: u64,
        total_bytes: u64,
        chunks_verified: u32,
        total_chunks: u32,
        speed_bytes_per_sec: f64,
        eta_seconds: f64,
        elapsed_ms: u64,
        connection_type: String,
    },
    PeerDiscovered {
        peer: MobilePeer,
    },
    PeerLost {
        node_id: String,
    },
    IncomingOffer {
        transfer_id: String,
        sender_node_id: String,
        sender_short_id: String,
        sender_name: String,
        file_count: u32,
        total_size: u64,
        files: Vec<MobileFileSummary>,
        connection_type: String,
    },
    ConnectionTypeChanged {
        transfer_id: String,
        peer_node_id: String,
        connection_type: String,
    },
    TransferPaused {
        transfer_id: String,
        direction: String,
    },
    TransferResumed {
        transfer_id: String,
        direction: String,
    },
    TransferCompleted {
        transfer_id: String,
        direction: String,
        total_bytes: u64,
        total_chunks: u32,
        elapsed_ms: u64,
        average_speed_bytes_per_sec: f64,
        output_paths: Vec<String>,
    },
    TransferFailed {
        transfer_id: String,
        direction: String,
        error: String,
    },
}

impl From<EngineEvent> for MobileEngineEvent {
    fn from(evt: EngineEvent) -> Self {
        match evt {
            EngineEvent::TransferProgress {
                transfer_id,
                direction,
                bytes_transferred,
                total_bytes,
                chunks_verified,
                total_chunks,
                speed_bytes_per_sec,
                eta_seconds,
                elapsed_ms,
                connection_type,
            } => Self::TransferProgress {
                transfer_id,
                direction,
                bytes_transferred,
                total_bytes,
                chunks_verified,
                total_chunks,
                speed_bytes_per_sec,
                eta_seconds,
                elapsed_ms,
                connection_type,
            },
            EngineEvent::PeerDiscovered { peer } => Self::PeerDiscovered {
                peer: MobilePeer::from(peer),
            },
            EngineEvent::PeerLost { node_id } => Self::PeerLost { node_id },
            EngineEvent::IncomingOffer {
                transfer_id,
                sender_node_id,
                sender_short_id,
                sender_name,
                file_count,
                total_size,
                files,
                connection_type,
            } => Self::IncomingOffer {
                transfer_id,
                sender_node_id,
                sender_short_id,
                sender_name,
                file_count,
                total_size,
                files: files.into_iter().map(MobileFileSummary::from).collect(),
                connection_type,
            },
            EngineEvent::ConnectionTypeChanged {
                transfer_id,
                peer_node_id,
                connection_type,
            } => Self::ConnectionTypeChanged {
                transfer_id,
                peer_node_id,
                connection_type,
            },
            EngineEvent::TransferPaused {
                transfer_id,
                direction,
            } => Self::TransferPaused {
                transfer_id,
                direction,
            },
            EngineEvent::TransferResumed {
                transfer_id,
                direction,
            } => Self::TransferResumed {
                transfer_id,
                direction,
            },
            EngineEvent::TransferCompleted {
                transfer_id,
                direction,
                total_bytes,
                total_chunks,
                elapsed_ms,
                average_speed_bytes_per_sec,
                output_paths,
            } => Self::TransferCompleted {
                transfer_id,
                direction,
                total_bytes,
                total_chunks,
                elapsed_ms,
                average_speed_bytes_per_sec,
                output_paths,
            },
            EngineEvent::TransferFailed {
                transfer_id,
                direction,
                error,
            } => Self::TransferFailed {
                transfer_id,
                direction,
                error,
            },
        }
    }
}

fn active_state() -> Result<MobileState, String> {
    MOBILE_ENGINE
        .read()
        .expect("mobile engine lock poisoned")
        .clone()
        .ok_or_else(|| "Veloce mobile engine is not initialized".to_string())
}

/// Initializes the Veloce mobile engine using bounded mobile memory defaults
/// (4 streams, 1 MiB chunk cap, buffered I/O without mmap, DNS relay resolution, and
/// optional platform-discovered LAN IP overrides for Android API 30+).
#[frb]
pub async fn init_mobile_engine(
    device_name: String,
    download_dir: String,
    relay_addrs: Vec<String>,
    profile: MobileTransferProfile,
    override_lan_addrs: Vec<String>,
    identity_key_path: Option<String>,
) -> Result<MobileNodeInfo, String> {
    // Shut down any previous instance cleanly when re-initializing (e.g. after settings change).
    shutdown();

    let engine = VeloceEngine::init(EngineConfigDto {
        device_name,
        bind_addr: None,
        enable_mdns: Some(true),
        parallel_streams: None,
        relay_addrs: if relay_addrs.is_empty() {
            None
        } else {
            Some(relay_addrs)
        },
        profile: Some(profile.into()),
        override_lan_addrs: if override_lan_addrs.is_empty() {
            None
        } else {
            Some(override_lan_addrs)
        },
        force_relay: Some(false),
        identity_key_path,
    })
    .await
    .map_err(|e| e.to_string())?;

    let info = MobileNodeInfo::from_dto(engine.node_info(), download_dir.clone());
    *MOBILE_ENGINE.write().expect("mobile engine lock poisoned") = Some(MobileState {
        engine,
        default_download_dir: download_dir,
    });
    Ok(info)
}

/// Streams all real-time `MobileEngineEvent` updates directly to Dart without blocking the UI isolate.
#[frb]
pub async fn subscribe_events(sink: StreamSink<MobileEngineEvent>) -> Result<(), String> {
    let state = active_state()?;
    let mut rx = state.engine.subscribe_events();
    loop {
        match rx.recv().await {
            Ok(evt) => {
                if sink.add(MobileEngineEvent::from(evt)).is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
            Err(broadcast::error::RecvError::Lagged(_)) => {}
        }
    }
    Ok(())
}

/// Starts hosting selected files/folders (generating a Global Session Ticket + QR URI)
/// and optionally initiates an immediate LAN push to `target_peer_node_id`.
#[frb]
pub async fn start_session(
    paths: Vec<String>,
    target_peer_node_id: Option<String>,
    relay_hints: Vec<String>,
) -> Result<MobileStartSessionResult, String> {
    let state = active_state()?;
    state
        .engine
        .start_session(paths, target_peer_node_id, relay_hints)
        .await
        .map(MobileStartSessionResult::from)
        .map_err(|e| e.to_string())
}

/// Joins a remote session using a `veloce1...` ticket string (or `veloce://...` QR URI).
/// Falls back to `download_dir` supplied during `init_mobile_engine` if `output_dir` is `None`.
#[frb]
pub async fn join_session(
    ticket_code: String,
    output_dir: Option<String>,
    parallel_streams: Option<u32>,
) -> Result<MobileJoinSessionResult, String> {
    let state = active_state()?;
    let target_dir = output_dir
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(state.default_download_dir);
    state
        .engine
        .join_session(ticket_code, target_dir, parallel_streams)
        .await
        .map(MobileJoinSessionResult::from)
        .map_err(|e| e.to_string())
}

/// Validates and decodes a `veloce1...` ticket for immediate UI preview without network I/O.
#[frb(sync)]
pub fn parse_ticket(ticket_code: String) -> Result<MobileTicketPreview, String> {
    VeloceEngine::parse_ticket(&ticket_code)
        .map(MobileTicketPreview::from)
        .map_err(|e| e.to_string())
}

/// Lists nearby LAN peers currently discovered over mDNS (`_veloce._udp.local.`).
#[frb(sync)]
pub fn list_local_peers() -> Result<Vec<MobilePeer>, String> {
    let state = active_state()?;
    Ok(state
        .engine
        .list_local_peers()
        .into_iter()
        .map(MobilePeer::from)
        .collect())
}

/// Accepts an incoming LAN transfer offer and writes verified chunks into `output_dir`
/// (or the configured default download directory if `None`).
#[frb(sync)]
pub fn accept_incoming_transfer(
    transfer_id: String,
    output_dir: Option<String>,
) -> Result<(), String> {
    let state = active_state()?;
    let target_dir = output_dir
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(state.default_download_dir);
    state
        .engine
        .accept_incoming_transfer(&transfer_id, target_dir)
        .map_err(|e| e.to_string())
}

/// Rejects an incoming LAN transfer offer.
#[frb(sync)]
pub fn reject_incoming_transfer(
    transfer_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    let state = active_state()?;
    state
        .engine
        .reject_incoming_transfer(&transfer_id, reason)
        .map_err(|e| e.to_string())
}

/// Pauses an active transfer while atomically persisting `.veloce-resume-*.state`.
#[frb(sync)]
pub fn pause_transfer(transfer_id: String) -> Result<bool, String> {
    let state = active_state()?;
    Ok(state.engine.pause_transfer(&transfer_id))
}

/// Resumes a previously paused transfer from its verified chunk bitfield.
#[frb]
pub async fn resume_transfer(transfer_id: String) -> Result<bool, String> {
    let state = active_state()?;
    state
        .engine
        .resume_transfer(&transfer_id)
        .await
        .map_err(|e| e.to_string())
}

/// Cancels an active or paused transfer by ID.
#[frb(sync)]
pub fn cancel_transfer(transfer_id: String) -> Result<bool, String> {
    let state = active_state()?;
    Ok(state.engine.cancel_transfer(&transfer_id))
}

/// Cleanly shuts down the QUIC endpoint, mDNS daemon, and background tasks.
#[frb(sync)]
pub fn shutdown() {
    if let Some(state) = MOBILE_ENGINE
        .write()
        .expect("mobile engine lock poisoned")
        .take()
    {
        state.engine.shutdown();
    }
}
