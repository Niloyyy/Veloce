use thiserror::Error;

/// Errors originating from chunking, hashing, Merkle verification, or disk assembly.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("insufficient disk space while writing '{path}': {detail}")]
    DiskFull { path: String, detail: String },

    #[error("protocol error: {0}")]
    Protocol(#[from] core_protocol::ProtocolError),

    #[error("serialization error: {0}")]
    Serialization(#[from] postcard::Error),

    #[error("chunk {chunk_index} is out of range (total chunks: {total_chunks})")]
    ChunkOutOfRange { chunk_index: u32, total_chunks: u32 },

    #[error("chunk {chunk_index} size mismatch: expected {expected} bytes, got {actual}")]
    ChunkSizeMismatch {
        chunk_index: u32,
        expected: usize,
        actual: usize,
    },

    #[error("chunk {chunk_index} BLAKE3 hash verification failed")]
    HashMismatch { chunk_index: u32 },

    #[error("chunk {chunk_index} Merkle inclusion proof verification failed")]
    InvalidMerkleProof { chunk_index: u32 },

    #[error("transfer is incomplete: {verified} / {total} chunks verified")]
    IncompleteTransfer { verified: u32, total: u32 },

    #[error("no input files provided")]
    EmptyInput,

    #[error("invalid chunk size {0}: must be > 0")]
    InvalidChunkSize(u32),

    #[error("resume state incompatible with manifest: {0}")]
    ResumeStateMismatch(String),

    #[error("background disk task failed: {0}")]
    JoinError(String),
}

impl StorageError {
    /// Classifies an `std::io::Error` into `StorageError::DiskFull` when ENOSPC / EDQUOT is detected.
    pub fn from_io_with_path(err: std::io::Error, path: impl Into<String>) -> Self {
        let is_disk_full = matches!(err.kind(), std::io::ErrorKind::StorageFull)
            || matches!(err.raw_os_error(), Some(28) | Some(122) | Some(112)); // ENOSPC / EDQUOT / ERROR_DISK_FULL
        if is_disk_full {
            StorageError::DiskFull {
                path: path.into(),
                detail: err.to_string(),
            }
        } else {
            StorageError::Io(err)
        }
    }
}

pub type Result<T> = std::result::Result<T, StorageError>;
