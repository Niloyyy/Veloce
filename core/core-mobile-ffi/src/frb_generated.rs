// Pre-codegen `flutter_rust_bridge` v2 StreamSink stub.
//
// Running `flutter_rust_bridge_codegen generate` (configured via `flutter_rust_bridge.yaml`
// and `.github/workflows/mobile.yml`) overwrites this file with full FRB v2.13 C-ABI glue.
// Keeping this lightweight stub checked in ensures `cargo test --workspace`, `cargo clippy`,
// and `cargo check --target aarch64-linux-android -p core-mobile-ffi` compile and run cleanly
// even in environments without the Flutter SDK installed.

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
