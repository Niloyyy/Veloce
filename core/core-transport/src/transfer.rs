use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes, BytesMut};
use core_protocol::{
    encode_chunk_response_header, encode_postcard_data_frame, read_frame, write_frame,
    ChunkErrorCode, ConnectionType, ControlMessage, DataMessage, HandshakeStateMachine, NodeId,
    ProtocolError, TransferManifest, TransferProfile, ZeroCopyDataFrame, DATA_TAG_CHUNK_RESPONSE,
    DATA_TAG_POSTCARD_FALLBACK, MAX_CONTROL_FRAME_SIZE, MAX_DATA_FRAME_SIZE, PROTOCOL_VERSION,
};
use core_storage::{ChunkAssembler, ChunkBitfield, ChunkSource};
use quinn::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::error::{Result, TransportError};
use crate::identity::{peer_node_id, PeerIdentity};
use crate::lan_lane::{connect_lan_tls_stream, LanSessionRegistration, LanTcpLane, LanTlsStream};
use crate::magic_socket::MagicUdpSocket;

/// Default number of concurrent bidirectional QUIC streams used per transfer session
/// to saturate Long Fat Networks and eliminate head-of-line blocking.
pub const DEFAULT_PARALLEL_STREAMS: usize = 8;

/// Number of chunk requests pipelined in flight per QUIC stream to keep BDP saturated
/// on high-latency WAN links and overlap disk I/O with network transfer on gigabit LANs.
const STREAM_PIPELINE_DEPTH: usize = 4;

/// Upper bound on concurrent chunk disk-verification/write tasks across all streams,
/// keeping memory consumption strictly bounded even on multi-gigabyte transfers.
const MAX_CONCURRENT_DISK_WRITES: usize = 24;

/// Maximum per-chunk retry attempts before failing the transfer.
const MAX_CHUNK_RETRIES: u32 = 4;

/// Direction of a transfer session relative to the local node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    Send,
    Receive,
}

/// Real-time telemetry snapshot emitted throughout an active transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transfer_id: String,
    pub direction: TransferDirection,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub chunks_verified: u32,
    pub total_chunks: u32,
    pub speed_bytes_per_sec: f64,
    pub eta_seconds: f64,
    /// Elapsed milliseconds measured from when the first chunk started being sent/received.
    pub elapsed_ms: u64,
    pub connection_type: ConnectionType,
}

/// Final summary returned when a transfer completes successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferSummary {
    pub transfer_id: String,
    pub total_bytes: u64,
    pub total_chunks: u32,
    /// Total session elapsed milliseconds (including handshake, streaming, and file finalization).
    pub elapsed_ms: u64,
    /// Steady-state chunk streaming milliseconds (excluding handshake and final fsync/rename).
    pub data_elapsed_ms: u64,
    /// End-to-end average throughput across `elapsed_ms`.
    pub average_speed_bytes_per_sec: f64,
    /// Steady-state streaming throughput across `data_elapsed_ms`.
    pub steady_state_speed_bytes_per_sec: f64,
    pub finalized_paths: Vec<PathBuf>,
}

/// Configuration knobs for parallel QUIC chunk transfer.
#[derive(Debug, Clone)]
pub struct TransferConfig {
    pub parallel_streams: usize,
    pub device_name: String,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            parallel_streams: DEFAULT_PARALLEL_STREAMS,
            device_name: "Veloce Node".into(),
        }
    }
}

/// Executes the sender side of a transfer over an established QUIC connection.
///
/// Assumes the control stream (`ctrl_send`, `ctrl_recv`) has already exchanged `Hello`
/// (and that the `HandshakeStateMachine` is in `HelloExchanged`).
#[allow(clippy::too_many_arguments)]
pub async fn run_sender_session(
    transfer_id: String,
    conn: Connection,
    ctrl_send: quinn::SendStream,
    ctrl_recv: quinn::RecvStream,
    sm: HandshakeStateMachine,
    source: Arc<ChunkSource>,
    sender_name: String,
    connection_type_rx: Arc<Mutex<ConnectionType>>,
    progress_tx: Option<mpsc::UnboundedSender<TransferProgress>>,
    cancel: CancellationToken,
) -> Result<TransferSummary> {
    run_sender_session_with_lan_lane(
        transfer_id,
        conn,
        ctrl_send,
        ctrl_recv,
        sm,
        source,
        sender_name,
        None,
        connection_type_rx,
        progress_tx,
        cancel,
    )
    .await
}

/// Executes the sender side of a transfer, accepting chunk requests over both QUIC data streams
/// and optional kernel-offloaded (TSO/GRO) Ed25519 mutual-TLS 1.3 LAN TCP streams simultaneously.
#[allow(clippy::too_many_arguments)]
pub async fn run_sender_session_with_lan_lane(
    transfer_id: String,
    conn: Connection,
    mut ctrl_send: quinn::SendStream,
    mut ctrl_recv: quinn::RecvStream,
    mut sm: HandshakeStateMachine,
    source: Arc<ChunkSource>,
    sender_name: String,
    lan_lane: Option<&LanTcpLane>,
    connection_type_rx: Arc<Mutex<ConnectionType>>,
    progress_tx: Option<mpsc::UnboundedSender<TransferProgress>>,
    cancel: CancellationToken,
) -> Result<TransferSummary> {
    let manifest = source.manifest().clone();
    let total_bytes = manifest.total_size;
    let total_chunks = manifest.total_chunks;

    // Register `(peer_node_id, session_id)` on the LAN TCP lane BEFORE sending `Offer` so
    // fast LAN receivers can dial TLS 1.3 TCP streams immediately upon `Accept`.
    let mut lan_registration: Option<LanSessionRegistration> = match lan_lane {
        Some(lane) => peer_node_id(&conn)
            .ok()
            .map(|peer_id| lane.register_session(peer_id, manifest.session_id)),
        None => None,
    };

    let offer = ControlMessage::Offer {
        manifest: manifest.clone(),
        sender_name,
    };
    sm.on_send(&offer)?;
    write_frame(&mut ctrl_send, &offer, MAX_CONTROL_FRAME_SIZE).await?;

    // Wait for Accept, Reject, or Cancel on control stream.
    let accept_msg: ControlMessage = read_frame(&mut ctrl_recv, MAX_CONTROL_FRAME_SIZE)
        .await?
        .ok_or_else(|| {
            TransportError::Connection("control stream closed before acceptance".into())
        })?;

    let (resumed_chunks, initial_sent_bytes) = match accept_msg {
        ControlMessage::Accept { resume_bitfield } => {
            sm.on_receive(ControlMessage::Accept {
                resume_bitfield: resume_bitfield.clone(),
            })?;
            let (verified_cnt, verified_bytes) =
                match ChunkBitfield::from_bytes(total_chunks, &resume_bitfield) {
                    Some(bf) => {
                        let bytes: u64 = (0..total_chunks)
                            .filter(|&i| bf.is_set(i))
                            .filter_map(|i| manifest.locate_chunk(i).map(|(_, _, len)| len as u64))
                            .sum();
                        (bf.verified_count(), bytes)
                    }
                    None => (0, 0),
                };
            tracing::info!(
                transfer_id = %transfer_id,
                resumed_chunks = verified_cnt,
                total_chunks,
                "Peer accepted transfer"
            );
            (verified_cnt, verified_bytes)
        }
        ControlMessage::Reject { reason } => {
            let _ = sm.on_receive(ControlMessage::Reject {
                reason: reason.clone(),
            });
            return Err(TransportError::Rejected(reason));
        }
        ControlMessage::Cancel { reason } => {
            let _ = sm.on_receive(ControlMessage::Cancel {
                reason: reason.clone(),
            });
            return Err(TransportError::Cancelled(reason));
        }
        other => {
            return Err(TransportError::Protocol(
                core_protocol::ProtocolError::InvalidTransition {
                    state: sm.state().to_string(),
                    event: format!("{other:?}"),
                },
            ));
        }
    };

    let start = Instant::now();
    // Latched the exact moment the first chunk starts being read/sent over data streams.
    let first_chunk_start: Arc<OnceLock<Instant>> = Arc::new(OnceLock::new());
    let bytes_sent = Arc::new(AtomicU64::new(initial_sent_bytes));
    let chunks_sent = Arc::new(AtomicU32::new(resumed_chunks));

    if resumed_chunks > 0 {
        if let Some(ref tx) = progress_tx {
            emit_progress(
                tx,
                &transfer_id,
                TransferDirection::Send,
                initial_sent_bytes.min(total_bytes),
                0,
                total_bytes,
                resumed_chunks.min(total_chunks),
                total_chunks,
                start,
                *connection_type_rx.lock().expect("conn_type lock poisoned"),
            );
        }
    }

    // Spawn data-stream serving loop handling pipelined parallel chunk requests concurrently.
    let serve_cancel = cancel.child_token();
    let serve_cancel_bg = serve_cancel.clone();
    let serve_conn = conn.clone();
    let serve_source = Arc::clone(&source);
    let serve_bytes = Arc::clone(&bytes_sent);
    let serve_chunks = Arc::clone(&chunks_sent);
    let serve_first_chunk = Arc::clone(&first_chunk_start);
    let serve_progress = progress_tx.clone();
    let serve_id = transfer_id.clone();
    let serve_conn_type = Arc::clone(&connection_type_rx);

    let data_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = serve_cancel_bg.cancelled() => break,
                Some(lan_tls) = recv_lan_registration(&mut lan_registration) => {
                    let src = Arc::clone(&serve_source);
                    let bytes_counter = Arc::clone(&serve_bytes);
                    let chunks_counter = Arc::clone(&serve_chunks);
                    let first_chunk = Arc::clone(&serve_first_chunk);
                    let prog = serve_progress.clone();
                    let tid = serve_id.clone();
                    let ct = Arc::clone(&serve_conn_type);
                    let stream_cancel = serve_cancel_bg.clone();

                    tokio::spawn(async move {
                        serve_lan_tls_stream(
                            lan_tls,
                            src,
                            bytes_counter,
                            chunks_counter,
                            initial_sent_bytes,
                            total_bytes,
                            total_chunks,
                            first_chunk,
                            prog,
                            tid,
                            ct,
                            stream_cancel,
                        )
                        .await;
                    });
                }
                accepted = serve_conn.accept_bi() => {
                    let (mut send, recv) = match accepted {
                        Ok(streams) => streams,
                        Err(_) => break,
                    };
                    let src = Arc::clone(&serve_source);
                    let bytes_counter = Arc::clone(&serve_bytes);
                    let chunks_counter = Arc::clone(&serve_chunks);
                    let first_chunk = Arc::clone(&serve_first_chunk);
                    let prog = serve_progress.clone();
                    let tid = serve_id.clone();
                    let ct = Arc::clone(&serve_conn_type);
                    let stream_cancel = serve_cancel_bg.clone();

                    tokio::spawn(async move {
                        if src.has_active_mmaps() {
                            // Zero-copy inline fast path for memory-mapped desktop files:
                            // slices `Bytes` directly from `Arc<Mmap>` without `spawn_blocking` or `memcpy`.
                            let mut reader = QuicRecvReader::new(recv);
                            let mut write_vec: Vec<Bytes> = Vec::with_capacity(16);
                            while !stream_cancel.is_cancelled() {
                                let req = match reader.read_next_frame(MAX_DATA_FRAME_SIZE).await {
                                    Ok(Some(ZeroCopyDataFrame::Control(msg))) => msg,
                                    Ok(Some(_)) => continue,
                                    Ok(None) | Err(_) => break,
                                };
                                let indices = match req {
                                    DataMessage::ChunkRequest { chunk_index } => vec![chunk_index],
                                    DataMessage::ChunkBatchRequest { chunk_indices } => chunk_indices,
                                    _ => continue,
                                };

                                write_vec.clear();
                                for idx in indices {
                                    if stream_cancel.is_cancelled() {
                                        break;
                                    }
                                    let chunk_start = *first_chunk.get_or_init(Instant::now);
                                    match src.read_chunk_bytes(idx) {
                                        Ok(payload) => {
                                            let len = payload.data.len() as u64;
                                            let sent_so_far =
                                                bytes_counter.fetch_add(len, Ordering::Relaxed) + len;
                                            let done_chunks =
                                                chunks_counter.fetch_add(1, Ordering::Relaxed) + 1;
                                            if let Some(ref tx) = prog {
                                                emit_progress(
                                                    tx,
                                                    &tid,
                                                    TransferDirection::Send,
                                                    sent_so_far.min(total_bytes),
                                                    sent_so_far.saturating_sub(initial_sent_bytes),
                                                    total_bytes,
                                                    done_chunks.min(total_chunks),
                                                    total_chunks,
                                                    chunk_start,
                                                    *ct.lock().expect("conn_type lock poisoned"),
                                                );
                                            }
                                            let Ok(hdr) = encode_chunk_response_header(
                                                payload.chunk_index,
                                                &payload.hash,
                                                &payload.proof,
                                                payload.data.len(),
                                                MAX_DATA_FRAME_SIZE,
                                            ) else {
                                                return;
                                            };
                                            write_vec.push(hdr);
                                            write_vec.push(payload.data);
                                            // Flush every 4 chunks so the receiver starts verifying immediately.
                                            if write_vec.len() >= 8 {
                                                if send.write_all_chunks(&mut write_vec).await.is_err() {
                                                    return;
                                                }
                                                write_vec.clear();
                                            }
                                        }
                                        Err(err) => {
                                            if !write_vec.is_empty() {
                                                if send.write_all_chunks(&mut write_vec).await.is_err() {
                                                    return;
                                                }
                                                write_vec.clear();
                                            }
                                            let err_msg = DataMessage::ChunkError {
                                                chunk_index: idx,
                                                code: ChunkErrorCode::IoError,
                                                message: err.to_string(),
                                            };
                                            let Ok(encoded) =
                                                encode_postcard_data_frame(&err_msg, MAX_DATA_FRAME_SIZE)
                                            else {
                                                return;
                                            };
                                            if send.write_chunk(encoded).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                }

                                if !write_vec.is_empty()
                                    && send.write_all_chunks(&mut write_vec).await.is_err()
                                {
                                    return;
                                }
                            }
                            let _ = send.finish();
                        } else {
                            // Pipelined prefetch path for Mobile / unmapped `pread` sources:
                            // overlaps concurrent flash `pread` tasks with QUIC Wi-Fi transmission.
                            type PrefetchItem = (
                                u32,
                                tokio::task::JoinHandle<
                                    core_storage::Result<core_storage::ChunkBytesPayload>,
                                >,
                            );
                            let (pf_tx, mut pf_rx) = mpsc::channel::<PrefetchItem>(6);
                            let reader_src = Arc::clone(&src);
                            let reader_cancel = stream_cancel.clone();
                            let reader_first = Arc::clone(&first_chunk);

                            let req_reader = tokio::spawn(async move {
                                let mut reader = QuicRecvReader::new(recv);
                                while !reader_cancel.is_cancelled() {
                                    let req = match reader.read_next_frame(MAX_DATA_FRAME_SIZE).await {
                                        Ok(Some(ZeroCopyDataFrame::Control(msg))) => msg,
                                        Ok(Some(_)) => continue,
                                        Ok(None) | Err(_) => break,
                                    };
                                    let indices = match req {
                                        DataMessage::ChunkRequest { chunk_index } => vec![chunk_index],
                                        DataMessage::ChunkBatchRequest { chunk_indices } => {
                                            chunk_indices
                                        }
                                        _ => continue,
                                    };
                                    for idx in indices {
                                        if reader_cancel.is_cancelled() {
                                            return;
                                        }
                                        reader_first.get_or_init(Instant::now);
                                        let s = Arc::clone(&reader_src);
                                        let handle =
                                            tokio::task::spawn_blocking(move || s.read_chunk_bytes(idx));
                                        if pf_tx.send((idx, handle)).await.is_err() {
                                            return;
                                        }
                                    }
                                }
                            });

                            while let Some((idx, handle)) = pf_rx.recv().await {
                                if stream_cancel.is_cancelled() {
                                    break;
                                }
                                let chunk_start = *first_chunk.get_or_init(Instant::now);
                                match handle.await {
                                    Ok(Ok(payload)) => {
                                        let len = payload.data.len() as u64;
                                        let sent_so_far =
                                            bytes_counter.fetch_add(len, Ordering::Relaxed) + len;
                                        let done_chunks =
                                            chunks_counter.fetch_add(1, Ordering::Relaxed) + 1;
                                        if let Some(ref tx) = prog {
                                            emit_progress(
                                                tx,
                                                &tid,
                                                TransferDirection::Send,
                                                sent_so_far.min(total_bytes),
                                                sent_so_far.saturating_sub(initial_sent_bytes),
                                                total_bytes,
                                                done_chunks.min(total_chunks),
                                                total_chunks,
                                                chunk_start,
                                                *ct.lock().expect("conn_type lock poisoned"),
                                            );
                                        }
                                        let Ok(hdr) = encode_chunk_response_header(
                                            payload.chunk_index,
                                            &payload.hash,
                                            &payload.proof,
                                            payload.data.len(),
                                            MAX_DATA_FRAME_SIZE,
                                        ) else {
                                            break;
                                        };
                                        let mut chunks = [hdr, payload.data];
                                        if send.write_all_chunks(&mut chunks).await.is_err() {
                                            break;
                                        }
                                    }
                                    Ok(Err(err)) => {
                                        let err_msg = DataMessage::ChunkError {
                                            chunk_index: idx,
                                            code: ChunkErrorCode::IoError,
                                            message: err.to_string(),
                                        };
                                        let Ok(encoded) =
                                            encode_postcard_data_frame(&err_msg, MAX_DATA_FRAME_SIZE)
                                        else {
                                            break;
                                        };
                                        if send.write_chunk(encoded).await.is_err() {
                                            break;
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                            req_reader.abort();
                            let _ = send.finish();
                        }
                    });
                }
            }
        }
    });

    // Wait for TransferComplete or Cancel on control stream, or local cancellation.
    let result = tokio::select! {
        _ = cancel.cancelled() => {
            let cancel_msg = ControlMessage::Cancel { reason: "cancelled by sender".into() };
            let _ = write_frame(&mut ctrl_send, &cancel_msg, MAX_CONTROL_FRAME_SIZE).await;
            let _ = ctrl_send.finish();
            Err(TransportError::Cancelled("cancelled by sender".into()))
        }
        ctrl_res = read_frame::<_, ControlMessage>(&mut ctrl_recv, MAX_CONTROL_FRAME_SIZE) => {
            match ctrl_res? {
                Some(ControlMessage::TransferComplete { verified_chunks }) => {
                    sm.on_receive(ControlMessage::TransferComplete { verified_chunks })?;
                    let _ = ctrl_send.finish();
                    let chunk_origin = first_chunk_start.get().copied().unwrap_or(start);
                    let elapsed = chunk_origin.elapsed();
                    let elapsed_ms = elapsed.as_millis().max(1) as u64;
                    let sent_this_session = total_bytes.saturating_sub(initial_sent_bytes);
                    let avg_speed = (sent_this_session as f64) / elapsed.as_secs_f64().max(0.0001);
                    if let Some(ref tx) = progress_tx {
                        emit_progress(
                            tx,
                            &transfer_id,
                            TransferDirection::Send,
                            total_bytes,
                            sent_this_session,
                            total_bytes,
                            total_chunks,
                            total_chunks,
                            chunk_origin,
                            *connection_type_rx.lock().expect("conn_type lock poisoned"),
                        );
                    }
                    Ok(TransferSummary {
                        transfer_id,
                        total_bytes,
                        total_chunks,
                        elapsed_ms,
                        data_elapsed_ms: elapsed_ms,
                        average_speed_bytes_per_sec: avg_speed,
                        steady_state_speed_bytes_per_sec: avg_speed,
                        finalized_paths: Vec::new(),
                    })
                }
                Some(ControlMessage::Cancel { reason }) => {
                    Err(TransportError::Cancelled(reason))
                }
                Some(other) => Err(TransportError::Protocol(
                    core_protocol::ProtocolError::InvalidTransition {
                        state: sm.state().to_string(),
                        event: format!("{other:?}"),
                    },
                )),
                None => Err(TransportError::Connection(
                    "peer disconnected before transfer completed".into(),
                )),
            }
        }
    };

    serve_cancel.cancel();
    data_task.abort();
    result
}

/// Executes the receiver side of a transfer using the Desktop concurrency profile.
#[allow(clippy::too_many_arguments)]
pub async fn run_receiver_session(
    transfer_id: String,
    conn: Connection,
    ctrl_send: quinn::SendStream,
    ctrl_recv: quinn::RecvStream,
    sm: HandshakeStateMachine,
    manifest: TransferManifest,
    output_dir: PathBuf,
    parallel_streams: usize,
    connection_type_rx: Arc<Mutex<ConnectionType>>,
    progress_tx: Option<mpsc::UnboundedSender<TransferProgress>>,
    cancel: CancellationToken,
) -> Result<TransferSummary> {
    run_receiver_session_with_profile(
        transfer_id,
        conn,
        ctrl_send,
        ctrl_recv,
        sm,
        manifest,
        output_dir,
        parallel_streams,
        TransferProfile::Desktop,
        connection_type_rx,
        progress_tx,
        cancel,
    )
    .await
}

/// Executes the receiver side of a transfer honoring `TransferProfile` (`Desktop` vs `Mobile`)
/// for stream pipeline depth and concurrent disk verification/write bounds.
#[allow(clippy::too_many_arguments)]
pub async fn run_receiver_session_with_profile(
    transfer_id: String,
    conn: Connection,
    ctrl_send: quinn::SendStream,
    ctrl_recv: quinn::RecvStream,
    sm: HandshakeStateMachine,
    manifest: TransferManifest,
    output_dir: PathBuf,
    parallel_streams: usize,
    profile: TransferProfile,
    connection_type_rx: Arc<Mutex<ConnectionType>>,
    progress_tx: Option<mpsc::UnboundedSender<TransferProgress>>,
    cancel: CancellationToken,
) -> Result<TransferSummary> {
    run_receiver_session_with_lan_lane(
        transfer_id,
        conn,
        ctrl_send,
        ctrl_recv,
        sm,
        manifest,
        output_dir,
        parallel_streams,
        profile,
        None,
        connection_type_rx,
        progress_tx,
        cancel,
    )
    .await
}

/// Executes the receiver side of a transfer, automatically accelerating direct `ConnectionType::Lan`
/// sessions with parallel kernel-offloaded (TSO/GRO) Ed25519 mutual-TLS 1.3 TCP data streams and
/// falling back seamlessly to multi-stream QUIC (BBR) whenever TCP is unavailable or on WAN/Relay.
#[allow(clippy::too_many_arguments)]
pub async fn run_receiver_session_with_lan_lane(
    transfer_id: String,
    conn: Connection,
    mut ctrl_send: quinn::SendStream,
    mut ctrl_recv: quinn::RecvStream,
    mut sm: HandshakeStateMachine,
    manifest: TransferManifest,
    output_dir: PathBuf,
    parallel_streams: usize,
    profile: TransferProfile,
    local_identity: Option<&PeerIdentity>,
    connection_type_rx: Arc<Mutex<ConnectionType>>,
    progress_tx: Option<mpsc::UnboundedSender<TransferProgress>>,
    cancel: CancellationToken,
) -> Result<TransferSummary> {
    let pipeline_depth = profile.stream_pipeline_depth().max(1);
    let max_disk_writes = profile.max_concurrent_disk_writes().max(1);
    let _ = (STREAM_PIPELINE_DEPTH, MAX_CONCURRENT_DISK_WRITES);
    let session_start = Instant::now();
    let total_bytes = manifest.total_size;
    let total_chunks = manifest.total_chunks;

    let assembler = ChunkAssembler::open_async(output_dir, manifest.clone()).await?;
    let resume_bf = assembler.bitfield_snapshot();
    let initial_verified = resume_bf.verified_count();
    let missing = resume_bf.missing_indices();

    let accept = ControlMessage::Accept {
        resume_bitfield: resume_bf.as_bytes().to_vec(),
    };
    sm.on_send(&accept)?;
    write_frame(&mut ctrl_send, &accept, MAX_CONTROL_FRAME_SIZE).await?;

    let data_start = Instant::now();
    // Latched the exact moment the first chunk request begins transferring over QUIC.
    let first_chunk_start: Arc<OnceLock<Instant>> = Arc::new(OnceLock::new());
    let initial_bytes: u64 = (0..total_chunks)
        .filter(|&i| resume_bf.is_set(i))
        .filter_map(|i| manifest.locate_chunk(i).map(|(_, _, len)| len as u64))
        .sum();
    let bytes_received = Arc::new(AtomicU64::new(initial_bytes));

    if initial_verified > 0 {
        if let Some(ref tx) = progress_tx {
            emit_progress(
                tx,
                &transfer_id,
                TransferDirection::Receive,
                initial_bytes.min(total_bytes),
                0,
                total_bytes,
                initial_verified.min(total_chunks),
                total_chunks,
                data_start,
                *connection_type_rx.lock().expect("conn_type lock poisoned"),
            );
        }
    }

    // Monitor control stream for remote cancellation while data streams run.
    let remote_cancel = cancel.child_token();
    let ctrl_watch_cancel = remote_cancel.clone();
    let remote_cancel_reason: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let reason_clone = Arc::clone(&remote_cancel_reason);
    let ctrl_watcher = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = ctrl_watch_cancel.cancelled() => break,
                msg = read_frame::<_, ControlMessage>(&mut ctrl_recv, MAX_CONTROL_FRAME_SIZE) => {
                    match msg {
                        Ok(Some(ControlMessage::Cancel { reason })) => {
                            *reason_clone.lock().expect("reason lock poisoned") = Some(reason);
                            ctrl_watch_cancel.cancel();
                            break;
                        }
                        Ok(Some(ControlMessage::Ping(_))) | Ok(Some(ControlMessage::Pong(_))) => {}
                        Ok(None) | Err(_) => break,
                        _ => {}
                    }
                }
            }
        }
    });

    if !missing.is_empty() {
        let queue: Arc<Mutex<VecDeque<(u32, u32)>>> = Arc::new(Mutex::new(
            missing.into_iter().map(|idx| (idx, 0u32)).collect(),
        ));
        let disk_sem = Arc::new(Semaphore::new(max_disk_writes));
        let remote_addr = conn.remote_address();
        let is_direct_lan = *connection_type_rx.lock().expect("conn_type lock poisoned")
            == ConnectionType::Lan
            && !MagicUdpSocket::is_virtual_addr(remote_addr)
            && total_bytes >= 512 * 1024;

        // Probe the high-throughput Ed25519 mTLS 1.3 LAN TCP data lane when on direct LAN.
        let mut initial_lan_streams: Vec<Option<LanTlsStream>> = Vec::new();
        let mut lan_reconnect_ctx: Option<(std::net::SocketAddr, NodeId, [u8; 16], PeerIdentity)> =
            None;
        if is_direct_lan {
            if let (Some(id), Ok(sender_id)) = (local_identity, peer_node_id(&conn)) {
                if let Ok(first_stream) = connect_lan_tls_stream(
                    remote_addr,
                    sender_id,
                    manifest.session_id,
                    id,
                    profile,
                    Duration::from_millis(120),
                )
                .await
                {
                    let lan_workers = parallel_streams
                        .clamp(1, 4)
                        .min(total_chunks.max(1) as usize);
                    initial_lan_streams.push(Some(first_stream));
                    let mut extra = JoinSet::new();
                    for _ in 1..lan_workers {
                        let id_clone = id.clone();
                        let sid = manifest.session_id;
                        extra.spawn(async move {
                            connect_lan_tls_stream(
                                remote_addr,
                                sender_id,
                                sid,
                                &id_clone,
                                profile,
                                Duration::from_millis(250),
                            )
                            .await
                            .ok()
                        });
                    }
                    while let Some(res) = extra.join_next().await {
                        if let Ok(Some(s)) = res {
                            initial_lan_streams.push(Some(s));
                        }
                    }
                    lan_reconnect_ctx =
                        Some((remote_addr, sender_id, manifest.session_id, id.clone()));
                }
            }
        }

        let worker_count = if !initial_lan_streams.is_empty() {
            initial_lan_streams.len()
        } else {
            parallel_streams
                .max(1)
                .min(total_chunks.max(1) as usize)
        };

        let mut workers = Vec::with_capacity(worker_count);
        for worker_idx in 0..worker_count {
            let preconnected_lan = initial_lan_streams
                .get_mut(worker_idx)
                .and_then(Option::take);
            let w_lan_ctx = lan_reconnect_ctx.clone();
            let w_conn = conn.clone();
            let w_queue = Arc::clone(&queue);
            let w_assembler = Arc::clone(&assembler);
            let w_bytes = Arc::clone(&bytes_received);
            let w_first_chunk = Arc::clone(&first_chunk_start);
            let w_progress = progress_tx.clone();
            let w_tid = transfer_id.clone();
            let w_ct = Arc::clone(&connection_type_rx);
            let w_cancel = remote_cancel.clone();
            let w_sem = Arc::clone(&disk_sem);

            workers.push(tokio::spawn(async move {
                let mut client_io = match preconnected_lan {
                    Some(tls) => ClientDataIo::LanTls(Box::new(tls)),
                    None => {
                        let (send, recv) = w_conn.open_bi().await?;
                        ClientDataIo::Quic {
                            send,
                            reader: QuicRecvReader::new(recv),
                        }
                    }
                };
                let refill_watermark = (pipeline_depth / 2).max(1);
                let mut inflight: VecDeque<(u32, u32)> = VecDeque::with_capacity(pipeline_depth);
                let mut batch_req_indices: Vec<u32> = Vec::with_capacity(pipeline_depth);
                let mut disk_tasks: JoinSet<Result<()>> = JoinSet::new();

                loop {
                    if w_cancel.is_cancelled() {
                        return Err(TransportError::Cancelled("transfer cancelled".into()));
                    }

                    // Drain completed disk tasks early so failures surface immediately.
                    while let Some(joined) = disk_tasks.try_join_next() {
                        match joined {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => return Err(e),
                            Err(e) => return Err(TransportError::Connection(e.to_string())),
                        }
                    }

                    // Watermarked batch-fill: top up to `pipeline_depth` in one `ChunkBatchRequest`
                    // whenever in-flight requests drop to `<= pipeline_depth / 2`.
                    let want = if inflight.len() <= refill_watermark {
                        pipeline_depth.saturating_sub(inflight.len())
                    } else {
                        0
                    };
                    if want > 0 {
                        batch_req_indices.clear();
                        let mut popped: Vec<(u32, u32)> = Vec::with_capacity(want);
                        {
                            let mut q = w_queue.lock().expect("queue lock poisoned");
                            for _ in 0..want {
                                let Some(item) = q.pop_front() else {
                                    break;
                                };
                                batch_req_indices.push(item.0);
                                popped.push(item);
                            }
                        }

                        if !popped.is_empty() {
                            w_first_chunk.get_or_init(Instant::now);
                            let req_msg = if batch_req_indices.len() == 1 {
                                DataMessage::ChunkRequest {
                                    chunk_index: batch_req_indices[0],
                                }
                            } else {
                                DataMessage::ChunkBatchRequest {
                                    chunk_indices: batch_req_indices.clone(),
                                }
                            };
                            let encoded =
                                encode_postcard_data_frame(&req_msg, MAX_DATA_FRAME_SIZE)?;
                            if let Err(write_err) = client_io.write_request(encoded).await {
                                let mut iter = popped.into_iter();
                                if let Some((first_idx, first_att)) = iter.next() {
                                    requeue_item(
                                        &w_queue,
                                        first_idx,
                                        first_att,
                                        &w_cancel,
                                        write_err,
                                    )?;
                                }
                                {
                                    let mut q = w_queue.lock().expect("queue lock");
                                    for (idx, att) in iter {
                                        q.push_front((idx, att));
                                    }
                                    while let Some((idx, att)) = inflight.pop_front() {
                                        if att + 1 < MAX_CHUNK_RETRIES {
                                            q.push_back((idx, att + 1));
                                        }
                                    }
                                }
                                client_io =
                                    reopen_client_data_io(&w_conn, &w_lan_ctx, profile).await?;
                                continue;
                            }
                            for item in popped {
                                inflight.push_back(item);
                            }
                        }
                    }

                    let Some((expected_idx, attempt)) = inflight.pop_front() else {
                        break;
                    };

                    let resp = match client_io.read_next_frame(MAX_DATA_FRAME_SIZE).await {
                        Ok(Some(frame)) => frame,
                        Ok(None) => {
                            requeue_item(
                                &w_queue,
                                expected_idx,
                                attempt,
                                &w_cancel,
                                TransportError::Connection("data stream closed unexpectedly".into()),
                            )?;
                            while let Some((idx, att)) = inflight.pop_front() {
                                if att + 1 < MAX_CHUNK_RETRIES {
                                    w_queue.lock().expect("queue lock").push_back((idx, att + 1));
                                }
                            }
                            client_io =
                                reopen_client_data_io(&w_conn, &w_lan_ctx, profile).await?;
                            continue;
                        }
                        Err(err) => {
                            requeue_item(
                                &w_queue,
                                expected_idx,
                                attempt,
                                &w_cancel,
                                TransportError::Protocol(err),
                            )?;
                            while let Some((idx, att)) = inflight.pop_front() {
                                if att + 1 < MAX_CHUNK_RETRIES {
                                    w_queue.lock().expect("queue lock").push_back((idx, att + 1));
                                }
                            }
                            client_io =
                                reopen_client_data_io(&w_conn, &w_lan_ctx, profile).await?;
                            continue;
                        }
                    };

                    match resp {
                        ZeroCopyDataFrame::ChunkResponse {
                            chunk_index: resp_idx,
                            proof,
                            data,
                            ..
                        } => {
                            if resp_idx != expected_idx {
                                requeue_item(
                                    &w_queue,
                                    expected_idx,
                                    attempt,
                                    &w_cancel,
                                    TransportError::Connection(format!(
                                        "received mismatched chunk {resp_idx} for request {expected_idx}"
                                    )),
                                )?;
                                continue;
                            }

                            // Overlap BLAKE3 Merkle verification + positional disk write with QUIC reads.
                            let permit = Arc::clone(&w_sem)
                                .acquire_owned()
                                .await
                                .map_err(|_| TransportError::Cancelled("semaphore closed".into()))?;
                            let asm = Arc::clone(&w_assembler);
                            let q = Arc::clone(&w_queue);
                            let bytes = Arc::clone(&w_bytes);
                            let chunk_origin = w_first_chunk.get().copied().unwrap_or(data_start);
                            let prog = w_progress.clone();
                            let tid = w_tid.clone();
                            let ct = Arc::clone(&w_ct);
                            let cancel_tok = w_cancel.clone();

                            disk_tasks.spawn(async move {
                                let _permit = permit;
                                match asm.write_verified_chunk_bytes_async(expected_idx, data, proof).await {
                                    Ok(outcome) => {
                                        let done_bytes = bytes
                                            .fetch_add(outcome.bytes_written as u64, Ordering::Relaxed)
                                            + outcome.bytes_written as u64;
                                        if let Some(ref tx) = prog {
                                            emit_progress(
                                                tx,
                                                &tid,
                                                TransferDirection::Receive,
                                                done_bytes.min(total_bytes),
                                                done_bytes.saturating_sub(initial_bytes),
                                                total_bytes,
                                                outcome.verified_chunks,
                                                outcome.total_chunks,
                                                chunk_origin,
                                                *ct.lock().expect("conn_type lock poisoned"),
                                            );
                                        }
                                        Ok(())
                                    }
                                    Err(storage_err) => {
                                        if matches!(
                                            storage_err,
                                            core_storage::StorageError::DiskFull { .. }
                                        ) {
                                            cancel_tok.cancel();
                                            return Err(TransportError::Storage(storage_err));
                                        }
                                        if attempt + 1 < MAX_CHUNK_RETRIES && !cancel_tok.is_cancelled() {
                                            tracing::warn!(
                                                chunk_index = expected_idx,
                                                attempt,
                                                error = %storage_err,
                                                "Retrying failed chunk"
                                            );
                                            q.lock()
                                                .expect("queue lock poisoned")
                                                .push_back((expected_idx, attempt + 1));
                                            Ok(())
                                        } else {
                                            Err(TransportError::Storage(storage_err))
                                        }
                                    }
                                }
                            });
                        }
                        ZeroCopyDataFrame::Control(DataMessage::ChunkError { message, .. }) => {
                            requeue_item(
                                &w_queue,
                                expected_idx,
                                attempt,
                                &w_cancel,
                                TransportError::Connection(format!(
                                    "sender returned chunk error: {message}"
                                )),
                            )?;
                        }
                        ZeroCopyDataFrame::Control(_) => {}
                    }
                }

                // Wait for all disk writes spawned by this stream worker to finish.
                while let Some(joined) = disk_tasks.join_next().await {
                    match joined {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => return Err(e),
                        Err(e) => return Err(TransportError::Connection(e.to_string())),
                    }
                }

                client_io.finish().await;
                Ok::<(), TransportError>(())
            }));
        }

        let mut first_err: Option<TransportError> = None;
        for w in workers {
            match w.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                    remote_cancel.cancel();
                }
                Err(join_err) => {
                    if first_err.is_none() {
                        first_err = Some(TransportError::Connection(join_err.to_string()));
                    }
                    remote_cancel.cancel();
                }
            }
        }

        if let Some(err) = first_err {
            ctrl_watcher.abort();
            // Always flush resume state on pause/interruption so resumption skips verified chunks!
            let _ = assembler.persist_state();
            if let Some(reason) = remote_cancel_reason.lock().expect("reason lock").take() {
                return Err(TransportError::Cancelled(reason));
            }
            if cancel.is_cancelled() {
                let _ = write_frame(
                    &mut ctrl_send,
                    &ControlMessage::Cancel {
                        reason: "cancelled by receiver".into(),
                    },
                    MAX_CONTROL_FRAME_SIZE,
                )
                .await;
                let _ = ctrl_send.finish();
            }
            return Err(err);
        }
    }

    let chunk_origin = first_chunk_start.get().copied().unwrap_or(data_start);
    let data_elapsed = chunk_origin.elapsed();
    ctrl_watcher.abort();

    let finalized_paths = assembler.finalize_async().await?;
    let complete_msg = ControlMessage::TransferComplete {
        verified_chunks: total_chunks,
    };
    sm.on_send(&complete_msg)?;
    write_frame(&mut ctrl_send, &complete_msg, MAX_CONTROL_FRAME_SIZE).await?;
    let _ = ctrl_send.finish();
    // Wait briefly for the sender to process TransferComplete and close the connection cleanly.
    let _ = tokio::time::timeout(Duration::from_millis(25), conn.closed()).await;

    let total_elapsed = session_start.elapsed();
    let elapsed_ms = total_elapsed.as_millis().max(1) as u64;
    let data_elapsed_ms = data_elapsed.as_millis().max(1) as u64;
    let transferred_this_session = total_bytes.saturating_sub(initial_bytes);
    let avg_speed = (transferred_this_session as f64) / total_elapsed.as_secs_f64().max(0.0001);
    let steady_speed = (transferred_this_session as f64) / data_elapsed.as_secs_f64().max(0.0001);

    if let Some(ref tx) = progress_tx {
        emit_progress(
            tx,
            &transfer_id,
            TransferDirection::Receive,
            total_bytes,
            transferred_this_session,
            total_bytes,
            total_chunks,
            total_chunks,
            chunk_origin,
            *connection_type_rx.lock().expect("conn_type lock poisoned"),
        );
    }

    tracing::info!(
        transfer_id = %transfer_id,
        initial_verified,
        total_chunks,
        elapsed_ms,
        data_elapsed_ms,
        "Transfer received and verified"
    );

    Ok(TransferSummary {
        transfer_id,
        total_bytes,
        total_chunks,
        elapsed_ms,
        data_elapsed_ms,
        average_speed_bytes_per_sec: avg_speed,
        steady_state_speed_bytes_per_sec: steady_speed,
        finalized_paths,
    })
}

async fn recv_lan_registration(
    reg: &mut Option<LanSessionRegistration>,
) -> Option<LanTlsStream> {
    match reg {
        Some(r) => r.stream_rx.recv().await,
        None => std::future::pending().await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn serve_lan_tls_stream(
    mut tls_stream: LanTlsStream,
    src: Arc<ChunkSource>,
    bytes_counter: Arc<AtomicU64>,
    chunks_counter: Arc<AtomicU32>,
    initial_sent_bytes: u64,
    total_bytes: u64,
    total_chunks: u32,
    first_chunk: Arc<OnceLock<Instant>>,
    prog: Option<mpsc::UnboundedSender<TransferProgress>>,
    tid: String,
    ct: Arc<Mutex<ConnectionType>>,
    stream_cancel: CancellationToken,
) {
    let mut write_vec: Vec<Bytes> = Vec::with_capacity(16);
    while !stream_cancel.is_cancelled() {
        let req = match tls_stream.read_next_frame(MAX_DATA_FRAME_SIZE).await {
            Ok(Some(ZeroCopyDataFrame::Control(msg))) => msg,
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => break,
        };
        let indices = match req {
            DataMessage::ChunkRequest { chunk_index } => vec![chunk_index],
            DataMessage::ChunkBatchRequest { chunk_indices } => chunk_indices,
            _ => continue,
        };

        write_vec.clear();
        if src.has_active_mmaps() {
            for idx in indices {
                if stream_cancel.is_cancelled() {
                    break;
                }
                let chunk_start = *first_chunk.get_or_init(Instant::now);
                match src.read_chunk_bytes(idx) {
                    Ok(payload) => {
                        let len = payload.data.len() as u64;
                        let sent_so_far = bytes_counter.fetch_add(len, Ordering::Relaxed) + len;
                        let done_chunks = chunks_counter.fetch_add(1, Ordering::Relaxed) + 1;
                        if let Some(ref tx) = prog {
                            emit_progress(
                                tx,
                                &tid,
                                TransferDirection::Send,
                                sent_so_far.min(total_bytes),
                                sent_so_far.saturating_sub(initial_sent_bytes),
                                total_bytes,
                                done_chunks.min(total_chunks),
                                total_chunks,
                                chunk_start,
                                *ct.lock().expect("conn_type lock poisoned"),
                            );
                        }
                        let Ok(hdr) = encode_chunk_response_header(
                            payload.chunk_index,
                            &payload.hash,
                            &payload.proof,
                            payload.data.len(),
                            MAX_DATA_FRAME_SIZE,
                        ) else {
                            return;
                        };
                        write_vec.push(hdr);
                        write_vec.push(payload.data);
                        if write_vec.len() >= 4 {
                            if tls_stream.write_bytes_batch(&write_vec).await.is_err() {
                                return;
                            }
                            write_vec.clear();
                        }
                    }
                    Err(err) => {
                        if !write_vec.is_empty() {
                            if tls_stream.write_bytes_batch(&write_vec).await.is_err() {
                                return;
                            }
                            write_vec.clear();
                        }
                        let err_msg = DataMessage::ChunkError {
                            chunk_index: idx,
                            code: ChunkErrorCode::IoError,
                            message: err.to_string(),
                        };
                        let Ok(encoded) = encode_postcard_data_frame(&err_msg, MAX_DATA_FRAME_SIZE)
                        else {
                            return;
                        };
                        if tls_stream.write_bytes_batch(&[encoded]).await.is_err() {
                            return;
                        }
                    }
                }
            }
            if !write_vec.is_empty() && tls_stream.write_bytes_batch(&write_vec).await.is_err() {
                return;
            }
        } else {
            // Mobile / unmapped `pread` path: kick off all requested chunk reads in parallel!
            let mut handles = Vec::with_capacity(indices.len());
            for idx in indices {
                if stream_cancel.is_cancelled() {
                    break;
                }
                first_chunk.get_or_init(Instant::now);
                let s = Arc::clone(&src);
                handles.push((idx, tokio::task::spawn_blocking(move || s.read_chunk_bytes(idx))));
            }
            for (idx, handle) in handles {
                if stream_cancel.is_cancelled() {
                    return;
                }
                let chunk_start = *first_chunk.get_or_init(Instant::now);
                match handle.await {
                    Ok(Ok(payload)) => {
                        let len = payload.data.len() as u64;
                        let sent_so_far = bytes_counter.fetch_add(len, Ordering::Relaxed) + len;
                        let done_chunks = chunks_counter.fetch_add(1, Ordering::Relaxed) + 1;
                        if let Some(ref tx) = prog {
                            emit_progress(
                                tx,
                                &tid,
                                TransferDirection::Send,
                                sent_so_far.min(total_bytes),
                                sent_so_far.saturating_sub(initial_sent_bytes),
                                total_bytes,
                                done_chunks.min(total_chunks),
                                total_chunks,
                                chunk_start,
                                *ct.lock().expect("conn_type lock poisoned"),
                            );
                        }
                        let Ok(hdr) = encode_chunk_response_header(
                            payload.chunk_index,
                            &payload.hash,
                            &payload.proof,
                            payload.data.len(),
                            MAX_DATA_FRAME_SIZE,
                        ) else {
                            return;
                        };
                        if tls_stream
                            .write_bytes_batch(&[hdr, payload.data])
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Ok(Err(err)) => {
                        let err_msg = DataMessage::ChunkError {
                            chunk_index: idx,
                            code: ChunkErrorCode::IoError,
                            message: err.to_string(),
                        };
                        let Ok(encoded) = encode_postcard_data_frame(&err_msg, MAX_DATA_FRAME_SIZE)
                        else {
                            return;
                        };
                        if tls_stream.write_bytes_batch(&[encoded]).await.is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        }
    }
    let _ = tls_stream.finish().await;
}

enum ClientDataIo {
    Quic {
        send: quinn::SendStream,
        reader: QuicRecvReader,
    },
    LanTls(Box<LanTlsStream>),
}

impl ClientDataIo {
    async fn write_request(&mut self, encoded: Bytes) -> Result<()> {
        match self {
            ClientDataIo::Quic { send, .. } => send
                .write_chunk(encoded)
                .await
                .map_err(|e| TransportError::Connection(e.to_string())),
            ClientDataIo::LanTls(tls) => tls
                .write_bytes_batch(&[encoded])
                .await
                .map_err(|e| TransportError::Connection(e.to_string())),
        }
    }

    async fn read_next_frame(
        &mut self,
        max_size: usize,
    ) -> core_protocol::Result<Option<ZeroCopyDataFrame>> {
        match self {
            ClientDataIo::Quic { reader, .. } => reader.read_next_frame(max_size).await,
            ClientDataIo::LanTls(tls) => tls.read_next_frame(max_size).await,
        }
    }

    async fn finish(&mut self) {
        match self {
            ClientDataIo::Quic { send, .. } => {
                let _ = send.finish();
            }
            ClientDataIo::LanTls(tls) => {
                let _ = tls.finish().await;
            }
        }
    }
}

async fn reopen_client_data_io(
    conn: &Connection,
    lan_ctx: &Option<(std::net::SocketAddr, NodeId, [u8; 16], PeerIdentity)>,
    profile: TransferProfile,
) -> Result<ClientDataIo> {
    if let Some((addr, peer_id, session_id, identity)) = lan_ctx {
        if let Ok(tls) = connect_lan_tls_stream(
            *addr,
            *peer_id,
            *session_id,
            identity,
            profile,
            Duration::from_millis(250),
        )
        .await
        {
            return Ok(ClientDataIo::LanTls(Box::new(tls)));
        }
    }
    let (send, recv) = conn.open_bi().await?;
    Ok(ClientDataIo::Quic {
        send,
        reader: QuicRecvReader::new(recv),
    })
}

/// Rolling `BytesMut` reader over `quinn::RecvStream::read_chunk` that assembles `ZeroCopyDataFrame`
/// payloads directly into uninitialized spare capacity without `vec![0u8; chunk_size]` zero-fill `memset`s.
struct QuicRecvReader {
    recv: quinn::RecvStream,
    buf: BytesMut,
}

impl QuicRecvReader {
    fn new(recv: quinn::RecvStream) -> Self {
        Self {
            recv,
            buf: BytesMut::with_capacity(64 * 1024),
        }
    }

    async fn ensure_bytes(&mut self, needed: usize) -> core_protocol::Result<bool> {
        if self.buf.len() >= needed {
            return Ok(true);
        }
        self.buf.reserve(needed - self.buf.len());
        while self.buf.len() < needed {
            let max_pull = (needed - self.buf.len()).max(64 * 1024);
            let chunk = self
                .recv
                .read_chunk(max_pull, true)
                .await
                .map_err(|e| ProtocolError::Io(std::io::Error::other(e.to_string())))?;
            match chunk {
                Some(c) if !c.bytes.is_empty() => {
                    self.buf.extend_from_slice(&c.bytes);
                }
                _ => {
                    if self.buf.is_empty() {
                        return Ok(false);
                    }
                    return Err(ProtocolError::UnexpectedEof);
                }
            }
        }
        Ok(true)
    }

    async fn read_next_frame(
        &mut self,
        max_size: usize,
    ) -> core_protocol::Result<Option<ZeroCopyDataFrame>> {
        if !self.ensure_bytes(1).await? {
            return Ok(None);
        }
        let tag = self.buf.get_u8();

        match tag {
            DATA_TAG_CHUNK_RESPONSE => {
                if !self.ensure_bytes(38).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let chunk_index = self.buf.get_u32();
                let mut hash = [0u8; 32];
                self.buf.copy_to_slice(&mut hash);
                let proof_len = self.buf.get_u16() as usize;
                if proof_len > 64 {
                    return Err(ProtocolError::FrameTooLarge {
                        size: proof_len * 32,
                        max: 64 * 32,
                    });
                }

                let proof_bytes_len = proof_len * 32;
                if !self.ensure_bytes(proof_bytes_len + 4).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let mut proof = Vec::with_capacity(proof_len);
                for _ in 0..proof_len {
                    let mut node = [0u8; 32];
                    self.buf.copy_to_slice(&mut node);
                    proof.push(node);
                }
                let data_len = self.buf.get_u32() as usize;
                let total_len = 1 + 38 + proof_bytes_len + 4 + data_len;
                if total_len > max_size {
                    return Err(ProtocolError::FrameTooLarge {
                        size: total_len,
                        max: max_size,
                    });
                }

                if data_len > 0 && !self.ensure_bytes(data_len).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let data = self.buf.split_to(data_len).freeze();
                Ok(Some(ZeroCopyDataFrame::ChunkResponse {
                    chunk_index,
                    hash,
                    proof,
                    data,
                }))
            }
            DATA_TAG_POSTCARD_FALLBACK => {
                if !self.ensure_bytes(4).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let frame_len = self.buf.get_u32() as usize;
                if frame_len > max_size {
                    return Err(ProtocolError::FrameTooLarge {
                        size: frame_len,
                        max: max_size,
                    });
                }
                if frame_len > 0 && !self.ensure_bytes(frame_len).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let payload = self.buf.split_to(frame_len);
                let decoded: DataMessage = postcard::from_bytes(&payload)?;
                Ok(Some(ZeroCopyDataFrame::Control(decoded)))
            }
            other => Err(ProtocolError::InvalidSignalingPacket(format!(
                "unknown data frame tag 0x{other:02x}"
            ))),
        }
    }
}

fn requeue_item(
    queue: &Arc<Mutex<VecDeque<(u32, u32)>>>,
    chunk_index: u32,
    attempt: u32,
    cancel: &CancellationToken,
    err: TransportError,
) -> Result<()> {
    if attempt + 1 < MAX_CHUNK_RETRIES && !cancel.is_cancelled() {
        queue
            .lock()
            .expect("queue lock poisoned")
            .push_back((chunk_index, attempt + 1));
        Ok(())
    } else {
        Err(err)
    }
}

/// Helper exchanging `Hello` on the control stream for the dialer side.
pub async fn send_and_receive_hello(
    ctrl_send: &mut quinn::SendStream,
    ctrl_recv: &mut quinn::RecvStream,
    sm: &mut HandshakeStateMachine,
    local_node_id: NodeId,
    device_name: &str,
    session_token: Option<[u8; 16]>,
    expected_peer_node_id: Option<NodeId>,
) -> Result<(NodeId, String)> {
    let local_hello = ControlMessage::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: local_node_id,
        device_name: device_name.to_string(),
        session_token,
    };
    sm.on_send(&local_hello)?;
    write_frame(ctrl_send, &local_hello, MAX_CONTROL_FRAME_SIZE).await?;

    let remote_msg: ControlMessage = read_frame(ctrl_recv, MAX_CONTROL_FRAME_SIZE)
        .await?
        .ok_or_else(|| {
            TransportError::Connection("peer closed control stream during Hello".into())
        })?;

    let (peer_id, peer_name) = match &remote_msg {
        ControlMessage::Hello {
            node_id,
            device_name,
            ..
        } => (*node_id, device_name.clone()),
        ControlMessage::Cancel { reason } => return Err(TransportError::Cancelled(reason.clone())),
        other => {
            return Err(TransportError::Protocol(
                core_protocol::ProtocolError::InvalidTransition {
                    state: sm.state().to_string(),
                    event: format!("{other:?}"),
                },
            ));
        }
    };

    if let Some(expected) = expected_peer_node_id {
        if peer_id != expected {
            return Err(TransportError::PeerMismatch {
                expected: expected.to_base32(),
                actual: peer_id.to_base32(),
            });
        }
    }

    sm.on_receive(remote_msg)?;
    Ok((peer_id, peer_name))
}

#[allow(clippy::too_many_arguments)]
fn emit_progress(
    tx: &mpsc::UnboundedSender<TransferProgress>,
    transfer_id: &str,
    direction: TransferDirection,
    bytes_transferred: u64,
    session_bytes_delta: u64,
    total_bytes: u64,
    chunks_verified: u32,
    total_chunks: u32,
    start: Instant,
    connection_type: ConnectionType,
) {
    let elapsed = start.elapsed();
    let elapsed_ms = elapsed.as_millis() as u64;
    let elapsed_secs = elapsed.as_secs_f64().max(0.001);
    let speed = if session_bytes_delta > 0 {
        (session_bytes_delta as f64) / elapsed_secs
    } else {
        0.0
    };
    let remaining = total_bytes.saturating_sub(bytes_transferred) as f64;
    let eta = if speed > 1.0 { remaining / speed } else { 0.0 };

    let _ = tx.send(TransferProgress {
        transfer_id: transfer_id.to_string(),
        direction,
        bytes_transferred,
        total_bytes,
        chunks_verified,
        total_chunks,
        speed_bytes_per_sec: speed,
        eta_seconds: eta,
        elapsed_ms,
        connection_type,
    });
}

/// Gracefully closes a QUIC connection without artificial delay if already closed by the peer.
pub async fn graceful_close(conn: &Connection) {
    if conn.close_reason().is_none() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    conn.close(0u32.into(), b"done");
}
