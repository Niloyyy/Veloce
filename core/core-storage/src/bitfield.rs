use serde::{Deserialize, Serialize};

/// Compact bitfield tracking verified chunks for instant transfer resumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkBitfield {
    total_chunks: u32,
    bytes: Vec<u8>,
}

impl ChunkBitfield {
    /// Creates an empty bitfield for `total_chunks` chunks.
    pub fn new(total_chunks: u32) -> Self {
        let byte_len = (total_chunks as usize).div_ceil(8);
        Self {
            total_chunks,
            bytes: vec![0u8; byte_len],
        }
    }

    /// Reconstructs a bitfield from wire bytes, sanitizing trailing unused bits.
    pub fn from_bytes(total_chunks: u32, raw: &[u8]) -> Option<Self> {
        let byte_len = (total_chunks as usize).div_ceil(8);
        if raw.is_empty() {
            return Some(Self::new(total_chunks));
        }
        if raw.len() != byte_len {
            return None;
        }
        let mut bytes = raw.to_vec();
        let rem = (total_chunks as usize) % 8;
        if rem != 0 {
            let mask = (1u8 << rem) - 1;
            if let Some(last) = bytes.last_mut() {
                *last &= mask;
            }
        }
        Some(Self {
            total_chunks,
            bytes,
        })
    }

    pub fn total_chunks(&self) -> u32 {
        self.total_chunks
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn is_set(&self, index: u32) -> bool {
        if index >= self.total_chunks {
            return false;
        }
        let byte_idx = (index / 8) as usize;
        let bit = index % 8;
        (self.bytes[byte_idx] & (1u8 << bit)) != 0
    }

    /// Marks `index` as verified. Returns `true` if the bit transitioned from unset to set.
    pub fn set(&mut self, index: u32) -> bool {
        if index >= self.total_chunks {
            return false;
        }
        let byte_idx = (index / 8) as usize;
        let bit = index % 8;
        let mask = 1u8 << bit;
        let was_set = (self.bytes[byte_idx] & mask) != 0;
        self.bytes[byte_idx] |= mask;
        !was_set
    }

    pub fn clear(&mut self, index: u32) {
        if index >= self.total_chunks {
            return;
        }
        let byte_idx = (index / 8) as usize;
        let bit = index % 8;
        self.bytes[byte_idx] &= !(1u8 << bit);
    }

    pub fn verified_count(&self) -> u32 {
        self.bytes.iter().map(|b| b.count_ones()).sum()
    }

    pub fn is_complete(&self) -> bool {
        self.verified_count() == self.total_chunks
    }

    /// Returns all chunk indices not yet verified, in ascending order.
    pub fn missing_indices(&self) -> Vec<u32> {
        let mut missing = Vec::with_capacity((self.total_chunks - self.verified_count()) as usize);
        for idx in 0..self.total_chunks {
            if !self.is_set(idx) {
                missing.push(idx);
            }
        }
        missing
    }
}
