use std::net::SocketAddr;

use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Serialize};

use crate::error::{ProtocolError, Result};
use crate::types::NodeId;

/// Human-recognizable prefix for encoded Veloce session tickets.
pub const TICKET_PREFIX: &str = "veloce1";
const TICKET_FORMAT_VERSION: u8 = 1;
const CHECKSUM_LEN: usize = 4;

/// Compact, shareable global session descriptor (encoded as a string or QR code).
///
/// Contains everything a remote receiver needs to discover, hole-punch, or relay-connect
/// to the sender and cryptographically verify the incoming transfer's Merkle root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTicket {
    /// Ticket schema version.
    pub version: u8,
    /// Sender's Ed25519 public key (QUIC peer identity).
    pub node_id: NodeId,
    /// Direct socket address hints (LAN + reflexive public UDP endpoints).
    pub direct_addrs: Vec<SocketAddr>,
    /// Optional stateless relay endpoints (e.g., "relay.example.com:7878" or "1.2.3.4:7878").
    pub relay_hints: Vec<String>,
    /// BLAKE3 Merkle root of the transfer payload.
    pub merkle_root: [u8; 32],
    /// Ephemeral capability token authenticating this transfer session.
    pub session_token: [u8; 16],
    /// Preview metadata for UI display before connecting.
    pub total_size: u64,
    pub file_count: u32,
}

impl SessionTicket {
    pub fn new(
        node_id: NodeId,
        direct_addrs: Vec<SocketAddr>,
        relay_hints: Vec<String>,
        merkle_root: [u8; 32],
        session_token: [u8; 16],
        total_size: u64,
        file_count: u32,
    ) -> Self {
        Self {
            version: TICKET_FORMAT_VERSION,
            node_id,
            direct_addrs,
            relay_hints,
            merkle_root,
            session_token,
            total_size,
            file_count,
        }
    }

    /// Encodes the ticket into a checksum-protected `veloce1...` string suitable for copy/paste or QR codes.
    pub fn encode(&self) -> Result<String> {
        let payload = postcard::to_allocvec(self)?;
        let tag = compute_checksum(&payload);
        let mut buf = Vec::with_capacity(payload.len() + CHECKSUM_LEN);
        buf.extend_from_slice(&payload);
        buf.extend_from_slice(&tag);
        let encoded = BASE32_NOPAD.encode(&buf).to_ascii_lowercase();
        Ok(format!("{TICKET_PREFIX}{encoded}"))
    }

    /// Decodes and validates checksum + schema version from a `veloce1...` ticket string.
    pub fn decode(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        // Strip optional URI scheme (`veloce://veloce1...`) if scanned from a QR code.
        let body = trimmed
            .strip_prefix("veloce://")
            .unwrap_or(trimmed)
            .strip_prefix(TICKET_PREFIX)
            .ok_or_else(|| {
                ProtocolError::InvalidTicket(format!("missing '{TICKET_PREFIX}' prefix"))
            })?;

        // Ignore whitespace/dashes if formatted for readability.
        let cleaned: String = body
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase())
            .collect();

        let raw = BASE32_NOPAD
            .decode(cleaned.as_bytes())
            .map_err(|e| ProtocolError::InvalidTicket(format!("base32 decode failed: {e}")))?;

        if raw.len() <= CHECKSUM_LEN {
            return Err(ProtocolError::InvalidTicket(
                "ticket payload too short".into(),
            ));
        }

        let (payload, provided_tag) = raw.split_at(raw.len() - CHECKSUM_LEN);
        let expected_tag = compute_checksum(payload);
        if provided_tag != expected_tag {
            return Err(ProtocolError::InvalidTicket(
                "ticket checksum mismatch (corrupted or mistyped code)".into(),
            ));
        }

        let ticket: Self = postcard::from_bytes(payload)
            .map_err(|e| ProtocolError::InvalidTicket(format!("payload decode failed: {e}")))?;

        if ticket.version != TICKET_FORMAT_VERSION {
            return Err(ProtocolError::InvalidTicket(format!(
                "unsupported ticket version {}",
                ticket.version
            )));
        }

        Ok(ticket)
    }
}

fn compute_checksum(payload: &[u8]) -> [u8; CHECKSUM_LEN] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"VELOCE-TICKET-V1");
    hasher.update(payload);
    let digest = hasher.finalize();
    let mut out = [0u8; CHECKSUM_LEN];
    out.copy_from_slice(&digest.as_bytes()[..CHECKSUM_LEN]);
    out
}
