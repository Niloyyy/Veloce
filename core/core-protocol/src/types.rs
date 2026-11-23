use std::fmt;
use std::path::{Component, Path, PathBuf};

use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Serialize};

use crate::error::{ProtocolError, Result};

/// Current wire protocol version.
pub const PROTOCOL_VERSION: u16 = 1;

/// ALPN identifier negotiated over QUIC TLS 1.3.
pub const ALPN_VELOCE: &[u8] = b"veloce/1";

/// 32-byte Ed25519 public key identifying a peer node.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub [u8; 32]);

impl NodeId {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Encodes the NodeId as lowercase base32 without padding.
    pub fn to_base32(&self) -> String {
        BASE32_NOPAD.encode(&self.0).to_ascii_lowercase()
    }

    /// Parses a NodeId from base32 (case-insensitive) or 64-char hex.
    pub fn parse(s: &str) -> Result<Self> {
        let trimmed = s.trim();
        if trimmed.len() == 64 {
            if let Ok(bytes) = hex::decode(trimmed) {
                if bytes.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    return Ok(Self(arr));
                }
            }
        }
        let decoded = BASE32_NOPAD
            .decode(trimmed.to_ascii_uppercase().as_bytes())
            .map_err(|e| ProtocolError::InvalidNodeId(e.to_string()))?;
        if decoded.len() != 32 {
            return Err(ProtocolError::InvalidNodeId(format!(
                "expected 32 bytes, got {}",
                decoded.len()
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        Ok(Self(arr))
    }

    /// Short display fingerprint (first 8 base32 chars) for UI badges and logs.
    pub fn short_id(&self) -> String {
        let full = self.to_base32();
        full.chars().take(8).collect()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_base32())
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.short_id())
    }
}

/// Runtime resource tuning profile adapting memory, chunk ceiling, and concurrency
/// between desktop workstations and mobile devices (avoiding iOS Jetsam / Android LMK OOMs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferProfile {
    #[default]
    Desktop,
    Mobile,
}

impl TransferProfile {
    /// Default concurrent QUIC bidirectional data streams per transfer.
    pub const fn default_parallel_streams(self) -> usize {
        match self {
            TransferProfile::Desktop => 8,
            TransferProfile::Mobile => 4,
        }
    }

    /// Pipelined chunk requests in flight per QUIC stream.
    pub const fn stream_pipeline_depth(self) -> usize {
        match self {
            TransferProfile::Desktop => 4,
            TransferProfile::Mobile => 2,
        }
    }

    /// Maximum concurrent BLAKE3 verify + positional disk write tasks.
    pub const fn max_concurrent_disk_writes(self) -> usize {
        match self {
            TransferProfile::Desktop => 24,
            TransferProfile::Mobile => 6,
        }
    }

    /// Upper bound on chunk size in bytes (4 MiB desktop vs 1 MiB mobile).
    pub const fn max_chunk_size(self) -> u32 {
        match self {
            TransferProfile::Desktop => 4 * 1024 * 1024,
            TransferProfile::Mobile => 1024 * 1024,
        }
    }

    /// Whether to use `memmap2` read-only file mappings (disabled on mobile to avoid iOS Jetsam kills).
    pub const fn use_mmap(self) -> bool {
        match self {
            TransferProfile::Desktop => true,
            TransferProfile::Mobile => false,
        }
    }

    /// Connection-level QUIC flow-control receive window in bytes.
    pub const fn quic_receive_window(self) -> u32 {
        match self {
            TransferProfile::Desktop => 64 * 1024 * 1024,
            TransferProfile::Mobile => 8 * 1024 * 1024,
        }
    }

    /// Per-stream QUIC flow-control receive window in bytes.
    pub const fn quic_stream_receive_window(self) -> u32 {
        match self {
            TransferProfile::Desktop => 16 * 1024 * 1024,
            TransferProfile::Mobile => 2 * 1024 * 1024,
        }
    }

    /// Connection-level QUIC send window in bytes.
    pub const fn quic_send_window(self) -> u64 {
        match self {
            TransferProfile::Desktop => 64 * 1024 * 1024,
            TransferProfile::Mobile => 8 * 1024 * 1024,
        }
    }

    /// BBR initial congestion window in bytes.
    pub const fn bbr_initial_window(self) -> u64 {
        match self {
            TransferProfile::Desktop => 512 * 1024,
            TransferProfile::Mobile => 128 * 1024,
        }
    }

    /// Descending OS UDP socket buffer size ladder (`SO_RCVBUF`/`SO_SNDBUF`).
    pub const fn udp_buffer_ladder(self) -> &'static [usize] {
        match self {
            TransferProfile::Desktop => &[
                8 * 1024 * 1024,
                4 * 1024 * 1024,
                2 * 1024 * 1024,
                1024 * 1024,
            ],
            TransferProfile::Mobile => &[2 * 1024 * 1024, 1024 * 1024, 512 * 1024, 256 * 1024],
        }
    }
}

/// Active transport path classification reported to the UI and telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConnectionType {
    /// Direct local network path (mDNS / same subnet).
    Lan,
    /// Direct internet P2P UDP path (NAT hole-punched).
    DirectP2p,
    /// End-to-end encrypted fallback via stateless relay server.
    Relayed,
}

impl ConnectionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionType::Lan => "LAN",
            ConnectionType::DirectP2p => "Direct-P2P",
            ConnectionType::Relayed => "Relayed",
        }
    }
}

impl fmt::Display for ConnectionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Metadata for a single file within a transfer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Forward-slash normalized, traversal-safe relative path (e.g., "photos/vacation.jpg").
    pub relative_path: String,
    /// File length in bytes.
    pub size: u64,
    /// First global chunk index belonging to this file.
    pub first_chunk: u32,
    /// Number of chunks belonging to this file (`0` for empty files).
    pub chunk_count: u32,
    /// BLAKE3 hash of the entire file content.
    pub file_hash: [u8; 32],
    /// Optional Unix permission mode bits (lower 12 bits).
    pub mode: Option<u32>,
}

/// Content-addressed manifest describing all files and chunks in a transfer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferManifest {
    /// Random 128-bit identifier for this transfer session.
    pub session_id: [u8; 16],
    /// Ordered list of files included in the transfer.
    pub files: Vec<FileEntry>,
    /// Fixed chunk size in bytes (typically 1 MiB to 4 MiB).
    pub chunk_size: u32,
    /// Total byte size across all files.
    pub total_size: u64,
    /// Total number of chunks across all files.
    pub total_chunks: u32,
    /// BLAKE3 hash of each chunk indexed by global `chunk_index`.
    pub chunk_hashes: Vec<[u8; 32]>,
    /// BLAKE3 Merkle tree root over `chunk_hashes`.
    pub merkle_root: [u8; 32],
}

impl TransferManifest {
    /// Validates structural invariants of an untrusted manifest received over the network.
    pub fn validate(&self) -> Result<()> {
        if self.chunk_size == 0 {
            return Err(ProtocolError::InvalidManifest(
                "chunk_size must be > 0".into(),
            ));
        }
        if self.files.is_empty() {
            return Err(ProtocolError::InvalidManifest(
                "manifest must contain at least one file".into(),
            ));
        }
        if self.chunk_hashes.len() != self.total_chunks as usize {
            return Err(ProtocolError::InvalidManifest(format!(
                "chunk_hashes length {} does not match total_chunks {}",
                self.chunk_hashes.len(),
                self.total_chunks
            )));
        }

        let mut expected_chunk: u32 = 0;
        let mut computed_total_size: u64 = 0;
        let mut seen_paths = std::collections::HashSet::with_capacity(self.files.len());

        for file in &self.files {
            validate_relative_path(&file.relative_path)?;
            if !seen_paths.insert(file.relative_path.as_str()) {
                return Err(ProtocolError::InvalidManifest(format!(
                    "duplicate file path '{}'",
                    file.relative_path
                )));
            }
            if file.first_chunk != expected_chunk {
                return Err(ProtocolError::InvalidManifest(format!(
                    "file '{}' has first_chunk {}, expected {}",
                    file.relative_path, file.first_chunk, expected_chunk
                )));
            }
            let expected_count = if file.size == 0 {
                0
            } else {
                let count = file.size.div_ceil(self.chunk_size as u64);
                u32::try_from(count).map_err(|_| {
                    ProtocolError::InvalidManifest("chunk count overflows u32".into())
                })?
            };
            if file.chunk_count != expected_count {
                return Err(ProtocolError::InvalidManifest(format!(
                    "file '{}' has chunk_count {}, expected {}",
                    file.relative_path, file.chunk_count, expected_count
                )));
            }
            expected_chunk = expected_chunk.checked_add(file.chunk_count).ok_or_else(|| {
                ProtocolError::InvalidManifest("total chunk count overflows u32".into())
            })?;
            computed_total_size = computed_total_size.checked_add(file.size).ok_or_else(|| {
                ProtocolError::InvalidManifest("total_size overflows u64".into())
            })?;
        }

        if expected_chunk != self.total_chunks {
            return Err(ProtocolError::InvalidManifest(format!(
                "sum of file chunks {} does not match total_chunks {}",
                expected_chunk, self.total_chunks
            )));
        }
        if computed_total_size != self.total_size {
            return Err(ProtocolError::InvalidManifest(format!(
                "sum of file sizes {} does not match total_size {}",
                computed_total_size, self.total_size
            )));
        }
        Ok(())
    }

    /// Maps a global `chunk_index` to `(file_index, offset_within_file, expected_byte_len)`.
    pub fn locate_chunk(&self, chunk_index: u32) -> Option<(usize, u64, usize)> {
        if chunk_index >= self.total_chunks {
            return None;
        }
        // Binary search over non-empty files by finding the last file whose first_chunk <= chunk_index
        // and chunk_count > 0.
        let pos = self
            .files
            .partition_point(|f| f.first_chunk.saturating_add(f.chunk_count) <= chunk_index);
        let file = self.files.get(pos)?;
        if file.chunk_count == 0
            || chunk_index < file.first_chunk
            || chunk_index >= file.first_chunk + file.chunk_count
        {
            return None;
        }
        let local_idx = (chunk_index - file.first_chunk) as u64;
        let offset = local_idx * (self.chunk_size as u64);
        let remaining = file.size.saturating_sub(offset);
        let len = remaining.min(self.chunk_size as u64) as usize;
        Some((pos, offset, len))
    }
}

/// Validates that a relative transfer path is safe against directory traversal and OS-specific exploits.
pub fn validate_relative_path(raw: &str) -> Result<PathBuf> {
    if raw.is_empty() {
        return Err(ProtocolError::InvalidPath {
            path: raw.to_string(),
            reason: "path is empty".into(),
        });
    }
    if raw.contains('\0') {
        return Err(ProtocolError::InvalidPath {
            path: raw.to_string(),
            reason: "path contains NUL byte".into(),
        });
    }
    if raw.contains('\\') {
        return Err(ProtocolError::InvalidPath {
            path: raw.to_string(),
            reason: "backslashes are not allowed in normalized paths".into(),
        });
    }
    if raw.starts_with('/') {
        return Err(ProtocolError::InvalidPath {
            path: raw.to_string(),
            reason: "absolute paths are forbidden".into(),
        });
    }
    // Reject Windows drive letter prefixes like "C:foo" even on Unix hosts.
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(ProtocolError::InvalidPath {
            path: raw.to_string(),
            reason: "drive prefixes are forbidden".into(),
        });
    }

    let mut clean = PathBuf::new();
    for component in Path::new(raw).components() {
        match component {
            Component::Normal(seg) => {
                if seg.is_empty() {
                    return Err(ProtocolError::InvalidPath {
                        path: raw.to_string(),
                        reason: "empty path component".into(),
                    });
                }
                clean.push(seg);
            }
            Component::ParentDir | Component::CurDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ProtocolError::InvalidPath {
                    path: raw.to_string(),
                    reason: "traversal or root component is forbidden".into(),
                });
            }
        }
    }

    if clean.as_os_str().is_empty() {
        return Err(ProtocolError::InvalidPath {
            path: raw.to_string(),
            reason: "normalized path is empty".into(),
        });
    }

    Ok(clean)
}
