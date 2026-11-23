pub mod api;
pub mod frb_generated;

pub use api::*;

#[cfg(test)]
mod tests {
    use super::api::*;
    use crate::frb_generated::StreamSink;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn mobile_ffi_lifecycle_events_pause_resume_and_ticket_parse() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();
        let payload: Vec<u8> = (0..180_000u32).map(|i| ((i * 19) % 251) as u8).collect();
        let file_path = src_dir.path().join("mobile_test.bin");
        fs::write(&file_path, &payload).unwrap();

        let info = init_mobile_engine(
            "Pixel-9-Pro".into(),
            dst_dir.path().display().to_string(),
            vec![],
            MobileTransferProfile::Mobile,
            vec!["192.168.1.99".into()],
        )
        .await
        .unwrap();
        assert_eq!(info.device_name, "Pixel-9-Pro");
        assert!(!info.short_id.is_empty());

        let (sink, mut rx) = StreamSink::new_channel();
        subscribe_events(sink).unwrap();

        let started = start_session(vec![file_path.display().to_string()], None, vec![])
            .await
            .unwrap();
        assert_eq!(started.file_count, 1);
        assert_eq!(started.total_size, payload.len() as u64);
        assert!(started.chunk_size <= 1024 * 1024);

        let preview = parse_ticket(started.ticket_code.clone()).unwrap();
        assert_eq!(preview.total_size, payload.len() as u64);
        assert!(preview.direct_addrs.iter().any(|a| a.starts_with("192.168.1.99:")));

        // Verify pause -> resume emits typed MobileEngineEvent variants through StreamSink.
        assert!(pause_transfer(started.transfer_id.clone()).unwrap());
        let paused_evt = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(paused_evt, MobileEngineEvent::TransferPaused { .. }));

        assert!(resume_transfer(started.transfer_id.clone()).await.unwrap());
        let resumed_evt = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(resumed_evt, MobileEngineEvent::TransferResumed { .. }));

        assert!(cancel_transfer(started.transfer_id).unwrap());
        assert!(list_local_peers().is_ok());
        shutdown();
    }
}
