use thiserror::Error;

/// Transport-layer errors across QUIC, TLS, mDNS discovery, NAT traversal, and session transfer.
#[derive(Debug, Error)]
pub enum TransportError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol error: {0}")]
    Protocol(#[from] core_protocol::ProtocolError),

    #[error("storage error: {0}")]
    Storage(#[from] core_storage::StorageError),

    #[error("TLS / identity error: {0}")]
    Tls(String),

    #[error("QUIC connection error: {0}")]
    Connection(String),

    #[error("QUIC connect setup error: {0}")]
    Connect(#[from] quinn::ConnectError),

    #[error("QUIC write error: {0}")]
    Write(#[from] quinn::WriteError),

    #[error("QUIC read error: {0}")]
    Read(#[from] quinn::ReadError),

    #[error("mDNS discovery error: {0}")]
    Discovery(String),

    #[error("peer identity mismatch: expected {expected}, got {actual}")]
    PeerMismatch { expected: String, actual: String },

    #[error("manifest Merkle root mismatch with session ticket")]
    MerkleRootMismatch,

    #[error("session rejected by peer: {0}")]
    Rejected(String),

    #[error("transfer cancelled: {0}")]
    Cancelled(String),

    #[error("session not found or expired")]
    SessionNotFound,

    #[error("operation timed out: {0}")]
    Timeout(String),

    #[error("NAT traversal / relay failed: {0}")]
    NatTraversal(String),
}

impl From<quinn::ConnectionError> for TransportError {
    fn from(err: quinn::ConnectionError) -> Self {
        TransportError::Connection(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, TransportError>;
