pub mod assembler;
pub mod bitfield;
pub mod chunker;
pub mod error;
pub mod merkle;

pub use assembler::{ChunkAssembler, ChunkWriteOutcome, ResumeState};
pub use bitfield::ChunkBitfield;
pub use chunker::{
    select_chunk_size, select_chunk_size_for_profile, ChunkPayload, ChunkSource, MAX_CHUNK_SIZE,
    MID_CHUNK_SIZE, MIN_CHUNK_SIZE,
};
pub use error::{Result, StorageError};
pub use merkle::MerkleTree;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn dynamic_chunk_size_scales_with_transfer_size() {
        assert_eq!(select_chunk_size(1024), MIN_CHUNK_SIZE);
        assert_eq!(select_chunk_size(63 * 1024 * 1024), MIN_CHUNK_SIZE);
        assert_eq!(select_chunk_size(64 * 1024 * 1024), MID_CHUNK_SIZE);
        assert_eq!(select_chunk_size(1024 * 1024 * 1024), MAX_CHUNK_SIZE);
        // Mobile profile strictly caps chunks at 1 MiB regardless of payload size.
        assert_eq!(
            select_chunk_size_for_profile(
                2 * 1024 * 1024 * 1024,
                core_protocol::TransferProfile::Mobile
            ),
            MIN_CHUNK_SIZE
        );
    }

    #[test]
    fn mobile_profile_bypasses_mmap_and_reads_chunks_via_buffered_io() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("mobile_clip.bin");
        let payload: Vec<u8> = (0..150_000u32).map(|i| ((i * 5) % 251) as u8).collect();
        fs::write(&file_path, &payload).unwrap();

        let source = ChunkSource::from_paths_for_profile(
            &[&file_path],
            core_protocol::TransferProfile::Mobile,
            Some(2 * 1024 * 1024), // Must be clamped to 1 MiB mobile ceiling.
        )
        .unwrap();

        assert!(!source.has_active_mmaps());
        assert_eq!(source.manifest().chunk_size, MIN_CHUNK_SIZE);
        let chunk = source.read_chunk(0).unwrap();
        assert_eq!(chunk.data, payload);
    }

    #[test]
    fn merkle_tree_proofs_work_for_all_sizes_and_reject_tampering() {
        for count in [1usize, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 33] {
            let hashes: Vec<[u8; 32]> = (0..count)
                .map(|i| MerkleTree::hash_chunk_data(format!("chunk-{i}").as_bytes()))
                .collect();
            let tree = MerkleTree::from_chunk_hashes(&hashes);
            let root = tree.root();

            for (idx, hash) in hashes.iter().enumerate() {
                let proof = tree.generate_proof(idx as u32).unwrap();
                MerkleTree::verify_proof(&root, idx as u32, count as u32, hash, &proof).unwrap();

                // Verify chunk-index swapping fails whenever count > 1.
                if count > 1 {
                    let wrong_idx = ((idx + 1) % count) as u32;
                    assert!(
                        MerkleTree::verify_proof(&root, wrong_idx, count as u32, hash, &proof)
                            .is_err()
                    );
                }

                // Verify tampered content hash fails.
                let mut bad_hash = *hash;
                bad_hash[0] ^= 0xFF;
                assert!(
                    MerkleTree::verify_proof(&root, idx as u32, count as u32, &bad_hash, &proof)
                        .is_err()
                );
            }
        }
    }

    #[test]
    fn bitfield_operations_and_masking() {
        let mut bf = ChunkBitfield::new(10);
        assert_eq!(bf.verified_count(), 0);
        assert!(!bf.is_complete());
        assert_eq!(bf.missing_indices(), (0..10).collect::<Vec<_>>());

        assert!(bf.set(0));
        assert!(!bf.set(0)); // Duplicate set returns false.
        assert!(bf.set(9));
        assert_eq!(bf.verified_count(), 2);

        // Ensure unused high bits in wire bytes are masked off.
        let mut noisy = bf.as_bytes().to_vec();
        noisy[1] |= 0b1111_1100;
        let sanitized = ChunkBitfield::from_bytes(10, &noisy).unwrap();
        assert_eq!(sanitized.verified_count(), 2);
        assert!(!sanitized.is_set(10));
    }

    #[test]
    fn out_of_order_directory_transfer_with_resume_and_deduplication() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let payload_dir = src_dir.path().join("project_bundle");
        fs::create_dir_all(payload_dir.join("nested")).unwrap();

        let file_a_data: Vec<u8> = (0..180_000u32).map(|i| (i % 251) as u8).collect();
        let file_b_data: Vec<u8> = (0..95_000u32).map(|i| ((i * 7) % 251) as u8).collect();
        fs::write(payload_dir.join("alpha.bin"), &file_a_data).unwrap();
        fs::write(payload_dir.join("nested/beta.bin"), &file_b_data).unwrap();
        fs::write(payload_dir.join("nested/empty.txt"), b"").unwrap();

        // Use 32 KiB chunks to exercise multi-chunk + odd-tail Merkle proofs.
        let source = ChunkSource::from_paths(&[&payload_dir], Some(32 * 1024)).unwrap();
        let manifest = source.manifest().clone();
        assert_eq!(manifest.files.len(), 3);
        assert!(manifest.total_chunks >= 9);

        // Session 1: Deliver ONLY even chunks out-of-order, then simulate disconnect/restart.
        {
            let assembler = ChunkAssembler::open(dst_dir.path(), manifest.clone()).unwrap();
            let mut even_indices: Vec<u32> =
                (0..manifest.total_chunks).filter(|i| i % 2 == 0).collect();
            even_indices.reverse();

            for idx in even_indices {
                let chunk = source.read_chunk(idx).unwrap();
                let outcome = assembler
                    .write_verified_chunk(chunk.chunk_index, &chunk.data, &chunk.proof)
                    .unwrap();
                assert!(outcome.newly_verified);
            }

            // Corrupted chunk MUST be rejected without tainting the bitfield.
            let mut bad_chunk = source.read_chunk(1).unwrap();
            bad_chunk.data[0] ^= 0xAA;
            assert!(matches!(
                assembler.write_verified_chunk(
                    bad_chunk.chunk_index,
                    &bad_chunk.data,
                    &bad_chunk.proof
                ),
                Err(StorageError::HashMismatch { chunk_index: 1 })
            ));

            assembler.persist_state().unwrap();
            assert!(!assembler.is_complete());
        }

        // Session 2: Re-open assembler and verify instant resume skips all even chunks!
        {
            let assembler = ChunkAssembler::open(dst_dir.path(), manifest.clone()).unwrap();
            let missing = assembler.missing_chunks();
            let expected_odd: Vec<u32> =
                (0..manifest.total_chunks).filter(|i| i % 2 == 1).collect();
            assert_eq!(missing, expected_odd);

            // Re-sending an already verified chunk is deduplicated cleanly.
            let dup = source.read_chunk(0).unwrap();
            let dup_outcome = assembler
                .write_verified_chunk(dup.chunk_index, &dup.data, &dup.proof)
                .unwrap();
            assert!(!dup_outcome.newly_verified);
            assert_eq!(dup_outcome.bytes_written, 0);

            // Deliver remaining odd chunks in reverse order.
            for idx in missing.into_iter().rev() {
                let chunk = source.read_chunk(idx).unwrap();
                let outcome = assembler
                    .write_verified_chunk(chunk.chunk_index, &chunk.data, &chunk.proof)
                    .unwrap();
                assert!(outcome.newly_verified);
            }

            assert!(assembler.is_complete());
            let finalized = assembler.finalize().unwrap();
            assert_eq!(finalized.len(), 3);
        }

        // Verify bit-for-bit equality of all transferred files.
        assert_eq!(
            fs::read(dst_dir.path().join("project_bundle/alpha.bin")).unwrap(),
            file_a_data
        );
        assert_eq!(
            fs::read(dst_dir.path().join("project_bundle/nested/beta.bin")).unwrap(),
            file_b_data
        );
        assert_eq!(
            fs::read(dst_dir.path().join("project_bundle/nested/empty.txt")).unwrap(),
            b""
        );
    }

    #[tokio::test]
    async fn async_source_and_assembler_parallel_chunk_writes() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();
        let path = src_dir.path().join("big.bin");
        let data: Vec<u8> = (0..512_000u32).map(|i| ((i * 13) % 256) as u8).collect();
        fs::write(&path, &data).unwrap();

        let source = ChunkSource::from_paths_async(vec![path], Some(64 * 1024))
            .await
            .unwrap();
        let manifest = source.manifest().clone();
        let assembler = ChunkAssembler::open_async(dst_dir.path().to_path_buf(), manifest.clone())
            .await
            .unwrap();

        // Spawn parallel out-of-order tasks to verify concurrency safety.
        let mut handles = Vec::new();
        for idx in (0..manifest.total_chunks).rev() {
            let s = Arc::clone(&source);
            let a = Arc::clone(&assembler);
            handles.push(tokio::spawn(async move {
                let chunk = s.read_chunk_async(idx).await.unwrap();
                a.write_verified_chunk_async(chunk.chunk_index, chunk.data, chunk.proof)
                    .await
                    .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assembler.finalize_async().await.unwrap();
        assert_eq!(fs::read(dst_dir.path().join("big.bin")).unwrap(), data);
    }
}
