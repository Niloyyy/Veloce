use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use core_protocol::{validate_relative_path, FileEntry, TransferManifest, TransferProfile};
use memmap2::Mmap;
use rand::RngCore;

use crate::error::{Result, StorageError};
use crate::merkle::MerkleTree;

/// 1 MiB chunk size for smaller transfers (< 64 MiB) and mobile ceiling.
pub const MIN_CHUNK_SIZE: u32 = 1024 * 1024;
/// 2 MiB chunk size for medium transfers (64 MiB .. 1 GiB).
pub const MID_CHUNK_SIZE: u32 = 2 * 1024 * 1024;
/// 4 MiB chunk size for large transfers (>= 1 GiB).
pub const MAX_CHUNK_SIZE: u32 = 4 * 1024 * 1024;

const MEDIUM_TRANSFER_THRESHOLD: u64 = 64 * 1024 * 1024;
const LARGE_TRANSFER_THRESHOLD: u64 = 1024 * 1024 * 1024;

/// Dynamically selects an optimal chunk size between 1 MiB and 4 MiB based on total transfer size.
pub fn select_chunk_size(total_bytes: u64) -> u32 {
    select_chunk_size_for_profile(total_bytes, TransferProfile::Desktop)
}

/// Dynamically selects chunk size capped by `profile.max_chunk_size()` (1 MiB on Mobile, 4 MiB on Desktop).
pub fn select_chunk_size_for_profile(total_bytes: u64, profile: TransferProfile) -> u32 {
    let base = if total_bytes >= LARGE_TRANSFER_THRESHOLD {
        MAX_CHUNK_SIZE
    } else if total_bytes >= MEDIUM_TRANSFER_THRESHOLD {
        MID_CHUNK_SIZE
    } else {
        MIN_CHUNK_SIZE
    };
    base.min(profile.max_chunk_size())
}

/// Chunk payload paired with its BLAKE3 content hash and Merkle inclusion proof.
#[derive(Debug, Clone)]
pub struct ChunkPayload {
    pub chunk_index: u32,
    pub hash: [u8; 32],
    pub proof: Vec<[u8; 32]>,
    pub data: Vec<u8>,
}

struct SourceFileHandle {
    path: PathBuf,
    size: u64,
    /// Zero-copy read-only memory map when available for non-empty files.
    mmap: Option<Mmap>,
}

/// Indexed, content-addressed sender-side file/directory source supporting concurrent
/// zero-copy chunk reads and Merkle proof generation.
pub struct ChunkSource {
    manifest: TransferManifest,
    merkle_tree: MerkleTree,
    files: Vec<SourceFileHandle>,
}

impl ChunkSource {
    /// Scans one or more file or directory paths using the default Desktop profile.
    pub fn from_paths<P: AsRef<Path>>(
        inputs: &[P],
        chunk_size_override: Option<u32>,
    ) -> Result<Self> {
        Self::from_paths_for_profile(inputs, TransferProfile::Desktop, chunk_size_override)
    }

    /// Scans one or more file or directory paths honoring `TransferProfile` memory constraints
    /// (bypassing `memmap2` and capping chunks at 1 MiB under `TransferProfile::Mobile`).
    pub fn from_paths_for_profile<P: AsRef<Path>>(
        inputs: &[P],
        profile: TransferProfile,
        chunk_size_override: Option<u32>,
    ) -> Result<Self> {
        if inputs.is_empty() {
            return Err(StorageError::EmptyInput);
        }
        if let Some(0) = chunk_size_override {
            return Err(StorageError::InvalidChunkSize(0));
        }

        let mut collected: Vec<(String, PathBuf, u64, Option<u32>)> = Vec::new();
        for input in inputs {
            let input_path = input.as_ref();
            let meta = fs::metadata(input_path)?;
            let base_name = input_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("transfer")
                .to_string();

            if meta.is_file() {
                let rel = normalize_rel_path(&base_name)?;
                collected.push((rel, input_path.to_path_buf(), meta.len(), unix_mode(&meta)));
            } else if meta.is_dir() {
                collect_dir_recursive(input_path, &base_name, &mut collected)?;
            }
        }

        if collected.is_empty() {
            return Err(StorageError::EmptyInput);
        }

        // Deterministic ordering across platforms and filesystems.
        collected.sort_by(|a, b| a.0.cmp(&b.0));

        // Disambiguate duplicate top-level filenames if caller passed two files with the same name.
        deduplicate_relative_paths(&mut collected);

        let total_size: u64 = collected.iter().map(|(_, _, sz, _)| *sz).sum();
        let chunk_size = chunk_size_override
            .map(|c| c.min(profile.max_chunk_size()))
            .unwrap_or_else(|| select_chunk_size_for_profile(total_size, profile));
        let use_mmap = profile.use_mmap();

        let mut entries = Vec::with_capacity(collected.len());
        let mut handles = Vec::with_capacity(collected.len());
        let mut chunk_hashes: Vec<[u8; 32]> = Vec::new();
        let mut next_chunk_index: u32 = 0;

        for (rel_path, disk_path, size, mode) in collected {
            let file = File::open(&disk_path)?;
            let mmap = if use_mmap {
                try_map_readonly(&file, size)
            } else {
                None
            };
            let mut file_hasher = blake3::Hasher::new();
            let first_chunk = next_chunk_index;
            let mut chunk_count: u32 = 0;

            if size > 0 {
                if let Some(ref mapped) = mmap {
                    let slice: &[u8] = mapped;
                    for chunk in slice.chunks(chunk_size as usize) {
                        file_hasher.update(chunk);
                        chunk_hashes.push(MerkleTree::hash_chunk_data(chunk));
                        chunk_count = chunk_count.checked_add(1).ok_or_else(|| {
                            core_protocol::ProtocolError::InvalidManifest(
                                "chunk count overflow".into(),
                            )
                        })?;
                    }
                } else {
                    // Fallback bounded buffer read for special/unmappable filesystems.
                    let mut reader = file;
                    let mut buf = vec![0u8; chunk_size as usize];
                    let mut remaining = size;
                    while remaining > 0 {
                        let to_read = remaining.min(chunk_size as u64) as usize;
                        read_exact_len(&mut reader, &mut buf[..to_read])?;
                        file_hasher.update(&buf[..to_read]);
                        chunk_hashes.push(MerkleTree::hash_chunk_data(&buf[..to_read]));
                        chunk_count = chunk_count.checked_add(1).ok_or_else(|| {
                            core_protocol::ProtocolError::InvalidManifest(
                                "chunk count overflow".into(),
                            )
                        })?;
                        remaining -= to_read as u64;
                    }
                }
            }

            next_chunk_index = next_chunk_index.checked_add(chunk_count).ok_or_else(|| {
                core_protocol::ProtocolError::InvalidManifest("total chunk count overflow".into())
            })?;

            entries.push(FileEntry {
                relative_path: rel_path,
                size,
                first_chunk,
                chunk_count,
                file_hash: *file_hasher.finalize().as_bytes(),
                mode,
            });
            handles.push(SourceFileHandle {
                path: disk_path,
                size,
                mmap,
            });
        }

        let merkle_tree = MerkleTree::from_chunk_hashes(&chunk_hashes);
        let mut session_id = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut session_id);

        let manifest = TransferManifest {
            session_id,
            files: entries,
            chunk_size,
            total_size,
            total_chunks: next_chunk_index,
            chunk_hashes,
            merkle_root: merkle_tree.root(),
        };
        manifest.validate()?;

        Ok(Self {
            manifest,
            merkle_tree,
            files: handles,
        })
    }

    /// Async wrapper running indexing on Tokio's blocking pool (Desktop profile).
    pub async fn from_paths_async(
        inputs: Vec<PathBuf>,
        chunk_size_override: Option<u32>,
    ) -> Result<Arc<Self>> {
        Self::from_paths_for_profile_async(inputs, TransferProfile::Desktop, chunk_size_override)
            .await
    }

    /// Async wrapper running indexing with explicit `TransferProfile` (e.g., `Mobile` buffered I/O).
    pub async fn from_paths_for_profile_async(
        inputs: Vec<PathBuf>,
        profile: TransferProfile,
        chunk_size_override: Option<u32>,
    ) -> Result<Arc<Self>> {
        tokio::task::spawn_blocking(move || {
            Self::from_paths_for_profile(&inputs, profile, chunk_size_override)
        })
        .await
        .map_err(|e| StorageError::JoinError(e.to_string()))?
        .map(Arc::new)
    }

    /// Returns whether any source file handle is currently memory-mapped.
    pub fn has_active_mmaps(&self) -> bool {
        self.files.iter().any(|f| f.mmap.is_some())
    }

    pub fn manifest(&self) -> &TransferManifest {
        &self.manifest
    }

    pub fn merkle_tree(&self) -> &MerkleTree {
        &self.merkle_tree
    }

    /// Reads the raw bytes, BLAKE3 hash, and Merkle inclusion proof for `chunk_index`.
    pub fn read_chunk(&self, chunk_index: u32) -> Result<ChunkPayload> {
        let Some((file_idx, offset, len)) = self.manifest.locate_chunk(chunk_index) else {
            return Err(StorageError::ChunkOutOfRange {
                chunk_index,
                total_chunks: self.manifest.total_chunks,
            });
        };

        let handle = &self.files[file_idx];
        let data = if let Some(ref mapped) = handle.mmap {
            let start = offset as usize;
            let end = start + len;
            if end <= mapped.len() {
                mapped[start..end].to_vec()
            } else {
                read_range_from_disk(&handle.path, offset, len)?
            }
        } else {
            let _ = handle.size;
            read_range_from_disk(&handle.path, offset, len)?
        };

        let hash = self.manifest.chunk_hashes[chunk_index as usize];
        let proof = self.merkle_tree.generate_proof(chunk_index)?;

        Ok(ChunkPayload {
            chunk_index,
            hash,
            proof,
            data,
        })
    }

    /// Async wrapper for non-blocking concurrent chunk reads across QUIC streams.
    pub async fn read_chunk_async(self: &Arc<Self>, chunk_index: u32) -> Result<ChunkPayload> {
        let source = Arc::clone(self);
        tokio::task::spawn_blocking(move || source.read_chunk(chunk_index))
            .await
            .map_err(|e| StorageError::JoinError(e.to_string()))?
    }
}

fn try_map_readonly(file: &File, size: u64) -> Option<Mmap> {
    if size == 0 {
        return None;
    }
    // SAFETY: We only create a read-only mapping of an immutable source file during transfer.
    // Callers validate slice bounds before reading and fall back to standard I/O if mapping fails.
    unsafe { Mmap::map(file).ok() }
}

fn read_range_from_disk(path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len];
    read_exact_len(&mut file, &mut buf)?;
    Ok(buf)
}

fn read_exact_len(file: &mut File, buf: &mut [u8]) -> Result<()> {
    let mut pos = 0;
    while pos < buf.len() {
        let n = file.read(&mut buf[pos..])?;
        if n == 0 {
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "source file truncated during read",
            )));
        }
        pos += n;
    }
    Ok(())
}

fn collect_dir_recursive(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, PathBuf, u64, Option<u32>)>,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        // Skip symlinks to avoid symlink escape or infinite directory cycles.
        if meta.file_type().is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = normalize_rel_path(&format!("{prefix}/{name}"))?;
        if meta.is_file() {
            out.push((rel, path, meta.len(), unix_mode(&meta)));
        } else if meta.is_dir() {
            collect_dir_recursive(&path, &rel, out)?;
        }
    }
    Ok(())
}

fn normalize_rel_path(path: &str) -> Result<String> {
    let normalized = path.replace('\\', "/");
    validate_relative_path(&normalized)?;
    Ok(normalized)
}

fn deduplicate_relative_paths(entries: &mut [(String, PathBuf, u64, Option<u32>)]) {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (rel, _, _, _) in entries.iter_mut() {
        let entry = counts.entry(rel.clone()).or_insert(0);
        *entry += 1;
        if *entry > 1 {
            let suffix = *entry;
            if let Some(dot) = rel.rfind('.') {
                rel.insert_str(dot, &format!("_{suffix}"));
            } else {
                rel.push_str(&format!("_{suffix}"));
            }
        }
    }
}

#[cfg(unix)]
fn unix_mode(meta: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(meta.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn unix_mode(_meta: &fs::Metadata) -> Option<u32> {
    None
}
