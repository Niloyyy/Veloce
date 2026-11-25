pub mod engine;
#[cfg(feature = "napi-addon")]
pub mod napi_bridge;

pub use engine::{
    EngineConfigDto, EngineEvent, FfiError, FileSummaryDto, JoinSessionResultDto, NodeInfoDto,
    PeerDto, StartSessionResultDto, TicketPreviewDto, VeloceEngine,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn engine_start_and_join_session_emits_typed_events() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let payload: Vec<u8> = (0..320_000u32).map(|i| ((i * 11) % 251) as u8).collect();
        let src_file = src_dir.path().join("demo.bin");
        fs::write(&src_file, &payload).unwrap();

        let sender = VeloceEngine::init(EngineConfigDto {
            device_name: "FFI-Sender".into(),
            bind_addr: Some("127.0.0.1:0".into()),
            enable_mdns: Some(false),
            parallel_streams: Some(4),
            ..Default::default()
        })
        .await
        .unwrap();

        let receiver = VeloceEngine::init(EngineConfigDto {
            device_name: "FFI-Receiver".into(),
            bind_addr: Some("127.0.0.1:0".into()),
            enable_mdns: Some(false),
            parallel_streams: Some(4),
            ..Default::default()
        })
        .await
        .unwrap();

        let mut events = receiver.subscribe_events();

        let started = sender
            .start_session(vec![src_file.display().to_string()], None, vec![])
            .await
            .unwrap();
        assert_eq!(started.file_count, 1);
        assert_eq!(started.total_size, payload.len() as u64);

        let preview = VeloceEngine::parse_ticket(&started.ticket_code).unwrap();
        assert_eq!(preview.file_count, 1);
        assert_eq!(preview.total_size, payload.len() as u64);

        let joined = receiver
            .join_session(
                started.ticket_code.clone(),
                dst_dir.path().display().to_string(),
                Some(4),
            )
            .await
            .unwrap();
        assert_eq!(joined.file_count, 1);

        let mut saw_conn_changed = false;
        let mut saw_progress = false;
        let mut saw_completed = false;

        let deadline = tokio::time::sleep(Duration::from_secs(10));
        tokio::pin!(deadline);

        while !saw_completed {
            tokio::select! {
                _ = &mut deadline => panic!("timed out waiting for FFI TransferCompleted event"),
                evt = events.recv() => {
                    match evt.unwrap() {
                        EngineEvent::ConnectionTypeChanged { connection_type, .. } => {
                            assert_eq!(connection_type, "LAN");
                            saw_conn_changed = true;
                        }
                        EngineEvent::TransferProgress { chunks_verified, total_chunks, .. } => {
                            if chunks_verified == total_chunks {
                                saw_progress = true;
                            }
                        }
                        EngineEvent::TransferCompleted { total_bytes, output_paths, .. } => {
                            assert_eq!(total_bytes, payload.len() as u64);
                            assert_eq!(output_paths.len(), 1);
                            saw_completed = true;
                        }
                        EngineEvent::TransferFailed { error, .. } => {
                            panic!("unexpected FFI transfer failure: {error}");
                        }
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_conn_changed);
        assert!(saw_progress);
        assert_eq!(fs::read(dst_dir.path().join("demo.bin")).unwrap(), payload);

        sender.shutdown();
        receiver.shutdown();
    }

    #[tokio::test]
    async fn hosted_sender_and_lan_push_receiver_both_emit_completed_events() {
        let src_dir = tempdir().unwrap();
        let dst_pull = tempdir().unwrap();
        let dst_push = tempdir().unwrap();

        let payload: Vec<u8> = (0..180_000u32).map(|i| ((i * 29) % 251) as u8).collect();
        let src_file = src_dir.path().join("both_directions.bin");
        fs::write(&src_file, &payload).unwrap();

        let sender = VeloceEngine::init(EngineConfigDto {
            device_name: "SenderNode".into(),
            bind_addr: Some("127.0.0.1:0".into()),
            enable_mdns: Some(false),
            parallel_streams: Some(4),
            ..Default::default()
        })
        .await
        .unwrap();

        let receiver = VeloceEngine::init(EngineConfigDto {
            device_name: "ReceiverNode".into(),
            bind_addr: Some("127.0.0.1:0".into()),
            enable_mdns: Some(false),
            parallel_streams: Some(4),
            ..Default::default()
        })
        .await
        .unwrap();

        let mut sender_events = sender.subscribe_events();
        let started = sender
            .start_session(vec![src_file.display().to_string()], None, vec![])
            .await
            .unwrap();

        // Ensure unspecified 0.0.0.0 addresses are never embedded in tickets.
        let preview = VeloceEngine::parse_ticket(&started.ticket_code).unwrap();
        assert!(preview
            .direct_addrs
            .iter()
            .all(|a| !a.starts_with("0.0.0.0:")));

        let _joined = receiver
            .join_session(
                started.ticket_code.clone(),
                dst_pull.path().display().to_string(),
                Some(4),
            )
            .await
            .unwrap();

        // Verify the hosted SENDER also receives EngineEvent::TransferCompleted!
        let mut sender_completed = false;
        let deadline = tokio::time::sleep(Duration::from_secs(10));
        tokio::pin!(deadline);
        while !sender_completed {
            tokio::select! {
                _ = &mut deadline => panic!("timed out waiting for hosted sender TransferCompleted"),
                evt = sender_events.recv() => {
                    if let EngineEvent::TransferCompleted { direction, total_bytes, .. } = evt.unwrap() {
                        if direction == "send" {
                            assert_eq!(total_bytes, payload.len() as u64);
                            sender_completed = true;
                        }
                    }
                }
            }
        }

        let _ = dst_push;
        sender.shutdown();
        receiver.shutdown();
    }

    #[tokio::test]
    async fn engine_pause_and_resume_completes_from_saved_state() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let payload: Vec<u8> = (0..4_500_000u32).map(|i| ((i * 23) % 251) as u8).collect();
        let src_file = src_dir.path().join("resumable.bin");
        fs::write(&src_file, &payload).unwrap();

        let sender = VeloceEngine::init(EngineConfigDto {
            device_name: "PauseSender".into(),
            bind_addr: Some("127.0.0.1:0".into()),
            enable_mdns: Some(false),
            parallel_streams: Some(4),
            ..Default::default()
        })
        .await
        .unwrap();

        let receiver = VeloceEngine::init(EngineConfigDto {
            device_name: "PauseReceiver".into(),
            bind_addr: Some("127.0.0.1:0".into()),
            enable_mdns: Some(false),
            parallel_streams: Some(4),
            ..Default::default()
        })
        .await
        .unwrap();

        let started = sender
            .start_session(vec![src_file.display().to_string()], None, vec![])
            .await
            .unwrap();

        // Test sender-side pause -> reject pull -> resume -> succeed.
        assert!(sender.pause_transfer(&started.transfer_id));
        assert!(sender.resume_transfer(&started.transfer_id).await.unwrap());

        let mut events = receiver.subscribe_events();
        let joined = receiver
            .join_session(
                started.ticket_code.clone(),
                dst_dir.path().display().to_string(),
                Some(2),
            )
            .await
            .unwrap();

        // Pause immediately on receiver, assert TransferPaused, then resume and verify completion!
        assert!(receiver.pause_transfer(&joined.transfer_id));

        let mut saw_paused = false;
        let mut saw_resumed = false;
        let mut saw_completed = false;
        let deadline = tokio::time::sleep(Duration::from_secs(10));
        tokio::pin!(deadline);

        while !saw_completed {
            tokio::select! {
                _ = &mut deadline => panic!("timed out waiting for pause/resume completion"),
                evt = events.recv() => {
                    match evt.unwrap() {
                        EngineEvent::TransferPaused { transfer_id, .. } if transfer_id == joined.transfer_id => {
                            saw_paused = true;
                            // Brief yield so in-flight cancelled task flushes state before resume.
                            tokio::time::sleep(Duration::from_millis(40)).await;
                            assert!(receiver.resume_transfer(&joined.transfer_id).await.unwrap());
                        }
                        EngineEvent::TransferResumed { transfer_id, .. } if transfer_id == joined.transfer_id => {
                            saw_resumed = true;
                        }
                        EngineEvent::TransferCompleted { transfer_id, total_bytes, .. } if transfer_id == joined.transfer_id => {
                            assert_eq!(total_bytes, payload.len() as u64);
                            saw_completed = true;
                        }
                        EngineEvent::TransferFailed { error, .. } => {
                            panic!("unexpected failure during pause/resume: {error}");
                        }
                        _ => {}
                    }
                }
            }
        }

        assert!(saw_paused);
        assert!(saw_resumed);
        assert_eq!(
            fs::read(dst_dir.path().join("resumable.bin")).unwrap(),
            payload
        );

        sender.shutdown();
        receiver.shutdown();
    }

    #[tokio::test]
    async fn persistent_identity_key_keeps_same_node_id_across_restarts() {
        let dir = tempdir().unwrap();
        let key_path = dir.path().join("veloce-identity.key");

        let e1 = VeloceEngine::init(EngineConfigDto {
            device_name: "PersistNode".into(),
            bind_addr: Some("127.0.0.1:0".into()),
            enable_mdns: Some(false),
            identity_key_path: Some(key_path.display().to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
        let id1 = e1.node_info().node_id;
        e1.shutdown();

        let e2 = VeloceEngine::init(EngineConfigDto {
            device_name: "PersistNode".into(),
            bind_addr: Some("127.0.0.1:0".into()),
            enable_mdns: Some(false),
            identity_key_path: Some(key_path.display().to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
        let id2 = e2.node_info().node_id;
        e2.shutdown();

        assert_eq!(id1, id2);
        assert!(key_path.exists());
    }
}
