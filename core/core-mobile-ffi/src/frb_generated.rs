// Pre-codegen `flutter_rust_bridge` v2 StreamSink stub.
//
// Running `flutter_rust_bridge_codegen generate` (configured via `flutter_rust_bridge.yaml`
// and `.github/workflows/mobile.yml`) overwrites this file with full FRB v2.13 C-ABI glue.
// Keeping this lightweight stub checked in ensures `cargo test --workspace`, `cargo clippy`,
// and Android cross-compilation checks run cleanly even without Flutter installed.

use tokio::sync::mpsc;

/// Typed event sink matching `flutter_rust_bridge` v2's `StreamSink<T>` surface.
#[derive(Clone, Debug)]
pub struct StreamSink<T> {
    tx: mpsc::UnboundedSender<T>,
}

impl<T> StreamSink<T> {
    /// Creates an in-process channel-backed `StreamSink` for Rust unit/integration testing.
    pub fn new_channel() -> (Self, mpsc::UnboundedReceiver<T>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    /// Pushes a typed event to the Flutter/Dart stream listener without blocking.
    pub fn add(&self, value: T) -> Result<(), String> {
        self.tx
            .send(value)
            .map_err(|_| "mobile event stream closed".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::StreamSink;
    use crate::api::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn mobile_ffi_stream_sink_receives_pause_and_resume_events() {
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
            None,
        )
        .await
        .unwrap();
        assert_eq!(info.device_name, "Pixel-9-Pro");
        assert!(!info.short_id.is_empty());

        let (sink, mut rx) = StreamSink::new_channel();
        let stream_task = tokio::spawn(subscribe_events(sink));
        tokio::time::sleep(Duration::from_millis(20)).await;

        let started = start_session(vec![file_path.display().to_string()], None, vec![])
            .await
            .unwrap();
        assert_eq!(started.file_count, 1);
        assert_eq!(started.total_size, payload.len() as u64);
        assert!(started.chunk_size <= 1024 * 1024);

        let preview = parse_ticket(started.ticket_code.clone()).unwrap();
        assert_eq!(preview.total_size, payload.len() as u64);
        assert!(preview
            .direct_addrs
            .iter()
            .any(|a| a.starts_with("192.168.1.99:")));

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
        assert!(matches!(
            resumed_evt,
            MobileEngineEvent::TransferResumed { .. }
        ));

        assert!(cancel_transfer(started.transfer_id).unwrap());
        assert!(list_local_peers().is_ok());
        shutdown();
        let _ = tokio::time::timeout(Duration::from_secs(2), stream_task).await;
    }
}
