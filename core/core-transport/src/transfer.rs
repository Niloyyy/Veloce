use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use core_protocol::{
    read_data_frame, read_frame, write_data_frame, write_frame, ChunkErrorCode, ConnectionType,
    ControlMessage, DataMessage, HandshakeStateMachine, NodeId, TransferManifest, TransferProfile,
    MAX_CONTROL_FRAME_SIZE, MAX_DATA_FRAME_SIZE, PROTOCOL_VERSION,
};
use core_storage::{ChunkAssembler, ChunkBitfield, ChunkSource};
use quinn::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::error::{Result, TransportError};

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
    mut ctrl_send: quinn::SendStream,
    mut ctrl_recv: quinn::RecvStream,
    mut sm: HandshakeStateMachine,
    source: Arc<ChunkSource>,
    sender_name: String,
    connection_type_rx: Arc<Mutex<ConnectionType>>,
    progress_tx: Option<mpsc::UnboundedSender<TransferProgress>>,
    cancel: CancellationToken,
) -> Result<TransferSummary> {
    let manifest = source.manifest().clone();
    let total_bytes = manifest.total_size;
    let total_chunks = manifest.total_chunks;

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

    match accept_msg {
        ControlMessage::Accept { resume_bitfield } => {
            sm.on_receive(ControlMessage::Accept {
                resume_bitfield: resume_bitfield.clone(),
            })?;
            let resumed_chunks = ChunkBitfield::from_bytes(total_chunks, &resume_bitfield)
                .map(|b| b.verified_count())
                .unwrap_or(0);
            tracing::info!(
                transfer_id = %transfer_id,
                resumed_chunks,
                total_chunks,
                "Peer accepted transfer"
            );
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
    }

    let start = Instant::now();
    // Latched the exact moment the first chunk starts being read/sent over data streams.
    let first_chunk_start: Arc<OnceLock<Instant>> = Arc::new(OnceLock::new());
    let bytes_sent = Arc::new(AtomicU64::new(0));
    let chunks_sent = Arc::new(AtomicU32::new(0));

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
                accepted = serve_conn.accept_bi() => {
                    let (mut send, mut recv) = match accepted {
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
                        while !stream_cancel.is_cancelled() {
                            let req: Option<DataMessage> = match read_data_frame(&mut recv, MAX_DATA_FRAME_SIZE).await {
                                Ok(r) => r,
                                Err(_) => break,
                            };
                            let Some(msg) = req else { break };
                            let indices = match msg {
                                DataMessage::ChunkRequest { chunk_index } => vec![chunk_index],
                                DataMessage::ChunkBatchRequest { chunk_indices } => chunk_indices,
                                _ => continue,
                            };

                            for idx in indices {
                                if stream_cancel.is_cancelled() {
                                    break;
                                }
                                let chunk_start = *first_chunk.get_or_init(Instant::now);
                                let resp = match src.read_chunk_async(idx).await {
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
                                                total_bytes,
                                                done_chunks.min(total_chunks),
                                                total_chunks,
                                                chunk_start,
                                                *ct.lock().expect("conn_type lock poisoned"),
                                            );
                                        }
                                        DataMessage::ChunkResponse {
                                            chunk_index: payload.chunk_index,
                                            hash: payload.hash,
                                            proof: payload.proof,
                                            data: payload.data,
                                        }
                                    }
                                    Err(err) => DataMessage::ChunkError {
                                        chunk_index: idx,
                                        code: ChunkErrorCode::IoError,
                                        message: err.to_string(),
                                    },
                                };

                                if write_data_frame(&mut send, &resp, MAX_DATA_FRAME_SIZE).await.is_err() {
                                    return;
                                }
                            }
                        }
                        let _ = send.finish();
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
                    let chunk_origin = first_chunk_start.get().copied().unwrap_or(start);
                    let elapsed = chunk_origin.elapsed();
                    let elapsed_ms = elapsed.as_millis().max(1) as u64;
                    let avg_speed = (total_bytes as f64) / elapsed.as_secs_f64().max(0.0001);
                    if let Some(ref tx) = progress_tx {
                        emit_progress(
                            tx,
                            &transfer_id,
                            TransferDirection::Send,
                            total_bytes,
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
    mut ctrl_send: quinn::SendStream,
    mut ctrl_recv: quinn::RecvStream,
    mut sm: HandshakeStateMachine,
    manifest: TransferManifest,
    output_dir: PathBuf,
    parallel_streams: usize,
    profile: TransferProfile,
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
        let worker_count = parallel_streams
            .max(1)
            .min(total_chunks.max(1) as usize);

        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
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
                let (mut send, mut recv) = w_conn.open_bi().await?;
                let mut inflight: VecDeque<(u32, u32)> = VecDeque::with_capacity(pipeline_depth);
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

                    // Fill pipeline window up to `pipeline_depth` requests ahead.
                    while inflight.len() < pipeline_depth {
                        let next_item = {
                            let mut q = w_queue.lock().expect("queue lock poisoned");
                            q.pop_front()
                        };
                        let Some((chunk_index, attempt)) = next_item else {
                            break;
                        };
                        let req = DataMessage::ChunkRequest { chunk_index };
                        w_first_chunk.get_or_init(Instant::now);
                        if let Err(err) = write_data_frame(&mut send, &req, MAX_DATA_FRAME_SIZE).await {
                            // Requeue failed request + any remaining in-flight items on stream reset.
                            requeue_item(&w_queue, chunk_index, attempt, &w_cancel, TransportError::Protocol(err))?;
                            while let Some((idx, att)) = inflight.pop_front() {
                                if att + 1 < MAX_CHUNK_RETRIES {
                                    w_queue.lock().expect("queue lock").push_back((idx, att + 1));
                                }
                            }
                            let reopened = w_conn.open_bi().await?;
                            send = reopened.0;
                            recv = reopened.1;
                            continue;
                        }
                        inflight.push_back((chunk_index, attempt));
                    }

                    let Some((expected_idx, attempt)) = inflight.pop_front() else {
                        break;
                    };

                    let resp: DataMessage = match read_data_frame(&mut recv, MAX_DATA_FRAME_SIZE).await {
                        Ok(Some(msg)) => msg,
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
                            let reopened = w_conn.open_bi().await?;
                            send = reopened.0;
                            recv = reopened.1;
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
                            let reopened = w_conn.open_bi().await?;
                            send = reopened.0;
                            recv = reopened.1;
                            continue;
                        }
                    };

                    match resp {
                        DataMessage::ChunkResponse {
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
                                match asm.write_verified_chunk_async(expected_idx, data, proof).await {
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
                        DataMessage::ChunkError { message, .. } => {
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
                        _ => {}
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

                let _ = send.finish();
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
    let _ = tokio::time::timeout(Duration::from_millis(150), conn.closed()).await;

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
    total_bytes: u64,
    chunks_verified: u32,
    total_chunks: u32,
    start: Instant,
    connection_type: ConnectionType,
) {
    let elapsed = start.elapsed();
    let elapsed_ms = elapsed.as_millis() as u64;
    let elapsed_secs = elapsed.as_secs_f64().max(0.001);
    let speed = (bytes_transferred as f64) / elapsed_secs;
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
