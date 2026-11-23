use crate::error::{Result, StorageError};

const LEAF_DOMAIN_TAG: u8 = 0x00;
const NODE_DOMAIN_TAG: u8 = 0x01;
const EMPTY_TREE_DOMAIN: &[u8] = b"VELOCE-MERKLE-EMPTY-V1";

/// Binary BLAKE3 Merkle tree built over content-addressed chunk hashes.
///
/// Uses explicit domain separation (`0x00` for leaves, `0x01` for internal nodes)
/// and deterministic index-bound proof verification so neither second-preimage
/// attacks nor chunk-index swapping attacks can succeed.
#[derive(Debug, Clone)]
pub struct MerkleTree {
    total_chunks: u32,
    levels: Vec<Vec<[u8; 32]>>,
    root: [u8; 32],
}

impl MerkleTree {
    /// Computes the content-addressed BLAKE3 hash of raw chunk bytes.
    pub fn hash_chunk_data(data: &[u8]) -> [u8; 32] {
        *blake3::hash(data).as_bytes()
    }

    /// Computes a domain-separated Merkle leaf hash from a chunk's content hash.
    pub fn hash_leaf(chunk_hash: &[u8; 32]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&[LEAF_DOMAIN_TAG]);
        hasher.update(chunk_hash);
        *hasher.finalize().as_bytes()
    }

    /// Computes a domain-separated internal node hash from its left and right children.
    pub fn hash_internal(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&[NODE_DOMAIN_TAG]);
        hasher.update(left);
        hasher.update(right);
        *hasher.finalize().as_bytes()
    }

    /// Builds a Merkle tree from ordered chunk content hashes.
    pub fn from_chunk_hashes(chunk_hashes: &[[u8; 32]]) -> Self {
        let total_chunks = chunk_hashes.len() as u32;
        if chunk_hashes.is_empty() {
            let root = *blake3::hash(EMPTY_TREE_DOMAIN).as_bytes();
            return Self {
                total_chunks: 0,
                levels: Vec::new(),
                root,
            };
        }

        let mut levels: Vec<Vec<[u8; 32]>> = Vec::new();
        let leaf_level: Vec<[u8; 32]> = chunk_hashes.iter().map(Self::hash_leaf).collect();
        levels.push(leaf_level);

        while levels.last().map_or(0, Vec::len) > 1 {
            let prev = levels.last().expect("levels is non-empty");
            let mut next = Vec::with_capacity(prev.len().div_ceil(2));
            let mut i = 0;
            while i < prev.len() {
                if i + 1 < prev.len() {
                    next.push(Self::hash_internal(&prev[i], &prev[i + 1]));
                } else {
                    // Promote odd trailing node upward.
                    next.push(prev[i]);
                }
                i += 2;
            }
            levels.push(next);
        }

        let root = levels
            .last()
            .and_then(|top| top.first().copied())
            .expect("non-empty tree has a root");

        Self {
            total_chunks,
            levels,
            root,
        }
    }

    pub fn root(&self) -> [u8; 32] {
        self.root
    }

    pub fn total_chunks(&self) -> u32 {
        self.total_chunks
    }

    /// Generates an O(log N) Merkle inclusion proof for `chunk_index`.
    pub fn generate_proof(&self, chunk_index: u32) -> Result<Vec<[u8; 32]>> {
        if chunk_index >= self.total_chunks {
            return Err(StorageError::ChunkOutOfRange {
                chunk_index,
                total_chunks: self.total_chunks,
            });
        }

        let mut proof = Vec::new();
        let mut idx = chunk_index as usize;

        for level in &self.levels[..self.levels.len().saturating_sub(1)] {
            if idx.is_multiple_of(2) {
                if idx + 1 < level.len() {
                    proof.push(level[idx + 1]);
                }
                // Otherwise idx was promoted without a right sibling.
            } else {
                proof.push(level[idx - 1]);
            }
            idx /= 2;
        }

        Ok(proof)
    }

    /// Verifies that `chunk_hash` belongs at `chunk_index` in a tree of size `total_chunks`
    /// committed by `expected_root`.
    pub fn verify_proof(
        expected_root: &[u8; 32],
        chunk_index: u32,
        total_chunks: u32,
        chunk_hash: &[u8; 32],
        proof: &[[u8; 32]],
    ) -> Result<()> {
        if total_chunks == 0 || chunk_index >= total_chunks {
            return Err(StorageError::ChunkOutOfRange {
                chunk_index,
                total_chunks,
            });
        }

        let mut acc = Self::hash_leaf(chunk_hash);
        let mut idx = chunk_index as usize;
        let mut level_len = total_chunks as usize;
        let mut proof_iter = proof.iter();

        while level_len > 1 {
            if idx.is_multiple_of(2) {
                if idx + 1 < level_len {
                    let Some(right_sibling) = proof_iter.next() else {
                        return Err(StorageError::InvalidMerkleProof { chunk_index });
                    };
                    acc = Self::hash_internal(&acc, right_sibling);
                }
            } else {
                let Some(left_sibling) = proof_iter.next() else {
                    return Err(StorageError::InvalidMerkleProof { chunk_index });
                };
                acc = Self::hash_internal(left_sibling, &acc);
            }
            idx /= 2;
            level_len = level_len.div_ceil(2);
        }

        if proof_iter.next().is_some() || &acc != expected_root {
            return Err(StorageError::InvalidMerkleProof { chunk_index });
        }

        Ok(())
    }
}
