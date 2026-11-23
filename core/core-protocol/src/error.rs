use thiserror::Error;

/// Errors produced by wire encoding, ticket parsing, path validation, or handshake state transitions.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("serialization error: {0}")]
    Serialization(#[from] postcard::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("frame length {size} exceeds maximum allowed {max}")]
    FrameTooLarge { size: usize, max: usize },

    #[error("unexpected EOF while reading protocol frame")]
    UnexpectedEof,

    #[error("invalid session ticket: {0}")]
    InvalidTicket(String),

    #[error("invalid node ID: {0}")]
    InvalidNodeId(String),

    #[error("invalid relative file path '{path}': {reason}")]
    InvalidPath { path: String, reason: String },

    #[error("unsupported protocol version {received} (expected {expected})")]
    UnsupportedVersion { expected: u16, received: u16 },

    #[error("invalid handshake transition from {state} on event {event}")]
    InvalidTransition { state: String, event: String },

    #[error("session token mismatch")]
    SessionTokenMismatch,

    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("invalid signaling packet: {0}")]
    InvalidSignalingPacket(String),
}

pub type Result<T> = std::result::Result<T, ProtocolError>;
