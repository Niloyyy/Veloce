use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_protocol::{validate_relative_path, TransferManifest};
use serde::{Deserialize, Serialize};

use crate::bitfield::ChunkBitfield;
use crate::error::{Result, StorageError};
use crate::merkle::MerkleTree;

const RESUME_STATE_VERSION: u8 = 1;

/// Persisted sidecar metadata enabling instant resumption across disconnects and restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeState {
    pub version: u8,
    pub merkle_root: [u8; 32],
    pub total_size: u64,
    pub chunk_size: u32,
    pub total_chunks: u32,
    pub bitfield_bytes: Vec<u8>,
}

/// Outcome of writing and verifying a single incoming chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkWriteOutcome {
    /// `true` if this chunk was newly verified (not a duplicate write).
    pub newly_verified: bool,
    /// Total verified chunks so far.
    pub verified_chunks: u32,
    /// Total chunks required for completion.
    pub total_chunks: u32,
    /// Number of bytes written in this chunk.
    pub bytes_written: usize,
}

struct TargetFile {
    final_path: PathBuf,
    part_path: PathBuf,
    file: File,
    size: u64,
    mode: Option<u32>,
}

/// Out-of-order, content-verified, resumable multi-file chunk assembler.
///
/// Uses positional writes (`pwrite` on Unix / `seek_write` on Windows) so parallel QUIC streams
/// can flush chunks out-of-order with bounded memory and zero RAM spikes on multi-GB files.
pub struct ChunkAssembler {
    manifest: TransferManifest,
    output_dir: PathBuf,
    state_path: PathBuf,
    targets: Vec<TargetFile>,
    bitfield: Mutex<ChunkBitfield>,
    persist_every: u32,
    persist_tracker: Mutex<(u32, Instant)>,
}

impl ChunkAssembler {
    /// Initializes (or resumes) a transfer into `output_dir`.
    ///
    /// Automatically loads any compatible `.veloce-resume` state file, validates existing
    /// `.part` files on disk for deduplication, and pre-allocates missing file extents.
    pub fn open(output_dir: impl AsRef<Path>, manifest: TransferManifest) -> Result<Self> {
        manifest.validate()?;
        let output_dir = output_dir.as_ref().to_path_buf();
        fs::create_dir_all(&output_dir)?;

        let root_prefix = hex::encode(&manifest.merkle_root[..8]);
        let state_path = output_dir.join(format!(".veloce-resume-{root_prefix}.state"));

        // Attempt to load persisted resume bitfield.
        let mut bitfield = match Self::load_resume_state(&state_path, &manifest) {
            Ok(Some(bf)) => bf,
            Ok(None) => ChunkBitfield::new(manifest.total_chunks),
            Err(err) => {
                tracing::warn!("Ignoring incompatible resume state at {:?}: {}", state_path, err);
                ChunkBitfield::new(manifest.total_chunks)
            }
        };

        let mut targets = Vec::with_capacity(manifest.files.len());
        for entry in &manifest.files {
            let safe_rel = validate_relative_path(&entry.relative_path)?;
            let final_path = output_dir.join(&safe_rel);
            if let Some(parent) = final_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let part_name = format!(
                ".{}.{}.veloce.part",
                final_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file"),
                root_prefix
            );
            let part_path = final_path
                .parent()
                .unwrap_or(&output_dir)
                .join(part_name);

            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&part_path)
                .map_err(|e| StorageError::from_io_with_path(e, part_path.display().to_string()))?;

            let current_len = file.metadata()?.len();
            if current_len != entry.size {
                file.set_len(entry.size).map_err(|e| {
                    StorageError::from_io_with_path(e, part_path.display().to_string())
                })?;
            }

            targets.push(TargetFile {
                final_path,
                part_path,
                file,
                size: entry.size,
                mode: entry.mode,
            });
        }

        // Verify any chunks marked in the loaded bitfield AND deduplicate already-present valid chunks
        // on disk (handles abrupt crashes before state persistence).
        reconcile_disk_chunks(&manifest, &targets, &mut bitfield)?;

        let assembler = Self {
            manifest,
            output_dir,
            state_path,
            targets,
            bitfield: Mutex::new(bitfield),
            persist_every: 32,
            persist_tracker: Mutex::new((0, Instant::now())),
        };
        assembler.persist_state()?;
        Ok(assembler)
    }

    /// Async wrapper opening/resuming on Tokio's blocking pool.
    pub async fn open_async(
        output_dir: PathBuf,
        manifest: TransferManifest,
    ) -> Result<Arc<Self>> {
        tokio::task::spawn_blocking(move || Self::open(output_dir, manifest))
            .await
            .map_err(|e| StorageError::JoinError(e.to_string()))?
            .map(Arc::new)
    }

    pub fn manifest(&self) -> &TransferManifest {
        &self.manifest
    }

    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    pub fn bitfield_snapshot(&self) -> ChunkBitfield {
        self.bitfield.lock().expect("bitfield mutex poisoned").clone()
    }

    pub fn missing_chunks(&self) -> Vec<u32> {
        self.bitfield
            .lock()
            .expect("bitfield mutex poisoned")
            .missing_indices()
    }

    pub fn verified_count(&self) -> u32 {
        self.bitfield
            .lock()
            .expect("bitfield mutex poisoned")
            .verified_count()
    }

    pub fn is_complete(&self) -> bool {
        self.bitfield
            .lock()
            .expect("bitfield mutex poisoned")
            .is_complete()
    }

    /// Verifies a chunk's BLAKE3 hash and Merkle inclusion proof, writes it out-of-order
    /// to its exact file offset, and updates the persistent resume bitfield.
    pub fn write_verified_chunk(
        &self,
        chunk_index: u32,
        data: &[u8],
        proof: &[[u8; 32]],
    ) -> Result<ChunkWriteOutcome> {
        let Some((file_idx, offset, expected_len)) = self.manifest.locate_chunk(chunk_index) else {
            return Err(StorageError::ChunkOutOfRange {
                chunk_index,
                total_chunks: self.manifest.total_chunks,
            });
        };

        if data.len() != expected_len {
            return Err(StorageError::ChunkSizeMismatch {
                chunk_index,
                expected: expected_len,
                actual: data.len(),
            });
        }

        // 1. Verify BLAKE3 content hash against manifest.
        let actual_hash = MerkleTree::hash_chunk_data(data);
        let expected_hash = self.manifest.chunk_hashes[chunk_index as usize];
        if actual_hash != expected_hash {
            return Err(StorageError::HashMismatch { chunk_index });
        }

        // 2. Verify Merkle inclusion proof against root commitment.
        MerkleTree::verify_proof(
            &self.manifest.merkle_root,
            chunk_index,
            self.manifest.total_chunks,
            &actual_hash,
            proof,
        )?;

        // 3. Deduplicate: if already verified, skip redundant disk write.
        {
            let bf = self.bitfield.lock().expect("bitfield mutex poisoned");
            if bf.is_set(chunk_index) {
                return Ok(ChunkWriteOutcome {
                    newly_verified: false,
                    verified_chunks: bf.verified_count(),
                    total_chunks: self.manifest.total_chunks,
                    bytes_written: 0,
                });
            }
        }

        // 4. Write chunk at exact offset using positional I/O (safe for concurrent out-of-order writers).
        let target = &self.targets[file_idx];
        write_all_at_offset(&target.file, offset, data).map_err(|e| {
            StorageError::from_io_with_path(e, target.part_path.display().to_string())
        })?;

        // 5. Mark chunk verified and periodically persist resume state atomically
        //    (every `persist_every` chunks, every 1 second, or upon final chunk).
        let (newly_verified, verified_chunks, should_persist) = {
            let mut bf = self.bitfield.lock().expect("bitfield mutex poisoned");
            let newly = bf.set(chunk_index);
            let verified = bf.verified_count();
            let mut tracker = self
                .persist_tracker
                .lock()
                .expect("persist tracker poisoned");
            tracker.0 += 1;
            let now = Instant::now();
            let persist = tracker.0 >= self.persist_every
                || now.duration_since(tracker.1) >= Duration::from_secs(1)
                || verified == self.manifest.total_chunks;
            if persist {
                tracker.0 = 0;
                tracker.1 = now;
            }
            (newly, verified, persist)
        };

        if should_persist {
            self.persist_state()?;
        }

        Ok(ChunkWriteOutcome {
            newly_verified,
            verified_chunks,
            total_chunks: self.manifest.total_chunks,
            bytes_written: data.len(),
        })
    }

    /// Async wrapper for writing and verifying chunks without blocking Tokio worker threads.
    pub async fn write_verified_chunk_async(
        self: &Arc<Self>,
        chunk_index: u32,
        data: Vec<u8>,
        proof: Vec<[u8; 32]>,
    ) -> Result<ChunkWriteOutcome> {
        let assembler = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            assembler.write_verified_chunk(chunk_index, &data, &proof)
        })
        .await
        .map_err(|e| StorageError::JoinError(e.to_string()))?
    }

    /// Atomically persists the current chunk bitfield to disk (write temp + rename).
    ///
    /// Mid-transfer checkpoints intentionally avoid per-checkpoint `fsync()` stalls because
    /// `reconcile_disk_chunks` validates and salvages any unrecorded chunks on restart; full
    /// file durability is enforced via `sync_all()` during `finalize()`.
    pub fn persist_state(&self) -> Result<()> {
        let bf_bytes = {
            let bf = self.bitfield.lock().expect("bitfield mutex poisoned");
            bf.as_bytes().to_vec()
        };
        let state = ResumeState {
            version: RESUME_STATE_VERSION,
            merkle_root: self.manifest.merkle_root,
            total_size: self.manifest.total_size,
            chunk_size: self.manifest.chunk_size,
            total_chunks: self.manifest.total_chunks,
            bitfield_bytes: bf_bytes,
        };
        let encoded = postcard::to_allocvec(&state)?;
        let tmp_path = self.state_path.with_extension("state.tmp");
        {
            let mut tmp = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&tmp_path)
                .map_err(|e| StorageError::from_io_with_path(e, tmp_path.display().to_string()))?;
            tmp.write_all(&encoded)
                .map_err(|e| StorageError::from_io_with_path(e, tmp_path.display().to_string()))?;
            tmp.flush()
                .map_err(|e| StorageError::from_io_with_path(e, tmp_path.display().to_string()))?;
        }
        fs::rename(&tmp_path, &self.state_path)?;
        Ok(())
    }

    /// Flushes and atomically promotes all `.part` files to their final paths, cleaning up resume state.
    pub fn finalize(&self) -> Result<Vec<PathBuf>> {
        let verified = self.verified_count();
        if verified != self.manifest.total_chunks {
            return Err(StorageError::IncompleteTransfer {
                verified,
                total: self.manifest.total_chunks,
            });
        }

        let mut finalized_paths = Vec::with_capacity(self.targets.len());
        for target in &self.targets {
            target.file.sync_all().map_err(|e| {
                StorageError::from_io_with_path(e, target.part_path.display().to_string())
            })?;
            #[cfg(unix)]
            if let Some(mode) = target.mode {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&target.part_path, fs::Permissions::from_mode(mode));
            }
            if target.part_path.exists() {
                fs::rename(&target.part_path, &target.final_path)?;
            }
            let _ = target.size;
            finalized_paths.push(target.final_path.clone());
        }

        if self.state_path.exists() {
            let _ = fs::remove_file(&self.state_path);
        }

        Ok(finalized_paths)
    }

    /// Async wrapper for transfer finalization.
    pub async fn finalize_async(self: &Arc<Self>) -> Result<Vec<PathBuf>> {
        let assembler = Arc::clone(self);
        tokio::task::spawn_blocking(move || assembler.finalize())
            .await
            .map_err(|e| StorageError::JoinError(e.to_string()))?
    }

    fn load_resume_state(
        path: &Path,
        manifest: &TransferManifest,
    ) -> Result<Option<ChunkBitfield>> {
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(path)?;
        let state: ResumeState = postcard::from_bytes(&raw)?;
        if state.version != RESUME_STATE_VERSION {
            return Err(StorageError::ResumeStateMismatch(format!(
                "unsupported state version {}",
                state.version
            )));
        }
        if state.merkle_root != manifest.merkle_root
            || state.total_chunks != manifest.total_chunks
            || state.chunk_size != manifest.chunk_size
            || state.total_size != manifest.total_size
        {
            return Err(StorageError::ResumeStateMismatch(
                "manifest metadata does not match saved resume state".into(),
            ));
        }
        let bf = ChunkBitfield::from_bytes(manifest.total_chunks, &state.bitfield_bytes)
            .ok_or_else(|| {
                StorageError::ResumeStateMismatch("corrupted bitfield length".into())
            })?;
        Ok(Some(bf))
    }
}

/// Scans existing `.part` files on disk to verify marked bits and salvage any valid chunks
/// written prior to an ungraceful shutdown.
fn reconcile_disk_chunks(
    manifest: &TransferManifest,
    targets: &[TargetFile],
    bitfield: &mut ChunkBitfield,
) -> Result<()> {
    let mut buf = vec![0u8; manifest.chunk_size as usize];
    for (file_idx, entry) in manifest.files.iter().enumerate() {
        if entry.chunk_count == 0 {
            continue;
        }
        let target = &targets[file_idx];
        let mut reader = File::open(&target.part_path)?;
        for local_idx in 0..entry.chunk_count {
            let chunk_index = entry.first_chunk + local_idx;
            let offset = (local_idx as u64) * (manifest.chunk_size as u64);
            let len = entry.size.saturating_sub(offset).min(manifest.chunk_size as u64) as usize;
            let slice = &mut buf[..len];

            reader.seek(SeekFrom::Start(offset))?;
            if reader.read_exact(slice).is_err() {
                bitfield.clear(chunk_index);
                continue;
            }

            // Skip hashing all-zero uninitialized sparse chunks unless bitfield claimed it was set
            // or the expected hash actually matches all zeros.
            let expected = manifest.chunk_hashes[chunk_index as usize];
            let actual = MerkleTree::hash_chunk_data(slice);
            if actual == expected {
                bitfield.set(chunk_index);
            } else {
                bitfield.clear(chunk_index);
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn write_all_at_offset(file: &File, mut offset: u64, mut data: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    while !data.is_empty() {
        let written = file.write_at(data, offset)?;
        if written == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "failed to write chunk bytes at offset",
            ));
        }
        data = &data[written..];
        offset += written as u64;
    }
    Ok(())
}

#[cfg(windows)]
fn write_all_at_offset(file: &File, mut offset: u64, mut data: &[u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !data.is_empty() {
        let written = file.seek_write(data, offset)?;
        if written == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "failed to write chunk bytes at offset",
            ));
        }
        data = &data[written..];
        offset += written as u64;
    }
    Ok(())
}
