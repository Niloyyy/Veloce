use std::fmt;
use std::net::SocketAddr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{ProtocolError, Result};
use crate::types::{NodeId, TransferManifest};

/// High-throughput byte-slice serde helper so `postcard` copies raw chunk/packet payloads
/// in a single `memcpy` instead of iterating element-by-element through `SerializeSeq`.
mod byte_vec_serde {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<u8>, D::Error> {
        struct ByteVecVisitor;

        impl serde::de::Visitor<'_> for ByteVecVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a raw byte slice")
            }

            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> std::result::Result<Self::Value, E> {
                Ok(v.to_vec())
            }

            fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> std::result::Result<Self::Value, E> {
                Ok(v)
            }
        }

        deserializer.deserialize_byte_buf(ByteVecVisitor)
    }
}

/// Maximum control frame size (16 MiB — accommodates manifests with hundreds of thousands of chunks).
pub const MAX_CONTROL_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// Maximum data frame size (8 MiB — accommodates up to 4 MiB chunks plus Merkle proofs and framing).
pub const MAX_DATA_FRAME_SIZE: usize = 8 * 1024 * 1024;

/// 4-byte magic prefix for Veloce UDP signaling / STUN / relay envelopes.
///
/// First byte `0x1B` (`0b0001_1011`) has bit 6 (`0x40`, QUIC fixed bit per RFC 9000) cleared,
/// guaranteeing zero collision with valid QUIC long or short header packets on the same UDP socket.
pub const SIGNALING_MAGIC: [u8; 4] = [0x1B, b'V', b'L', b'C'];

/// Messages exchanged on the primary bidirectional QUIC control stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Initial handshake greeting authenticated by the QUIC mTLS peer identity.
    Hello {
        protocol_version: u16,
        node_id: NodeId,
        device_name: String,
        /// Optional session capability token (required for Global ticket pulls, None for LAN push offers).
        session_token: Option<[u8; 16]>,
    },
    /// Sender offers a transfer manifest to the receiver.
    Offer {
        manifest: TransferManifest,
        sender_name: String,
    },
    /// Receiver accepts the transfer and supplies a bitfield of already-verified chunks for instant resume.
    Accept {
        resume_bitfield: Vec<u8>,
    },
    /// Receiver explicitly declines the transfer.
    Reject {
        reason: String,
    },
    /// Receiver confirms all chunks have been received and cryptographically verified.
    TransferComplete {
        verified_chunks: u32,
    },
    /// Either peer gracefully cancels the transfer session.
    Cancel {
        reason: String,
    },
    /// Liveness / RTT probe.
    Ping(u64),
    Pong(u64),
}

/// Error codes returned on data streams when a chunk request cannot be fulfilled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkErrorCode {
    OutOfRange,
    NotFound,
    IoError,
    Cancelled,
}

/// Messages exchanged on parallel QUIC data streams (zero head-of-line blocking).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataMessage {
    /// Receiver requests one or more chunks to be served on this stream.
    ChunkRequest { chunk_index: u32 },
    /// Pipelined batch request for multiple chunks on one stream.
    ChunkBatchRequest { chunk_indices: Vec<u32> },
    /// Sender delivers verified chunk payload together with its BLAKE3 hash and Merkle inclusion proof.
    ChunkResponse {
        chunk_index: u32,
        hash: [u8; 32],
        proof: Vec<[u8; 32]>,
        #[serde(with = "byte_vec_serde")]
        data: Vec<u8>,
    },
    /// Sender signals an error fulfilling a chunk request.
    ChunkError {
        chunk_index: u32,
        code: ChunkErrorCode,
        message: String,
    },
}

/// UDP signaling, STUN, hole-punching, and blind encrypted relay messages multiplexed with QUIC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalingMessage {
    /// STUN-like reflexive address discovery request sent to relay server.
    StunRequest { tx_id: u64 },
    /// Relay echoes back the peer's observed public UDP socket address.
    StunResponse { tx_id: u64, observed_addr: SocketAddr },
    /// Peer registers its NodeId and local LAN address hints with the stateless relay.
    RelayRegister {
        node_id: NodeId,
        local_addrs: Vec<SocketAddr>,
    },
    /// Acknowledgement of relay registration including observed public address.
    RelayRegistered {
        observed_addr: SocketAddr,
        ttl_secs: u32,
    },
    /// Peer looks up a target peer's reflexive + local endpoints and asks relay to coordinate hole-punching.
    PeerLookup {
        target_node_id: NodeId,
        requester_node_id: NodeId,
        requester_local_addrs: Vec<SocketAddr>,
    },
    /// Relay responds with target peer endpoints if currently registered.
    PeerEndpoints {
        target_node_id: NodeId,
        observed_addr: Option<SocketAddr>,
        local_addrs: Vec<SocketAddr>,
    },
    /// Relay pushes simultaneous hole-punch coordination hint to a registered peer.
    PunchNotify {
        from_node_id: NodeId,
        from_observed_addr: SocketAddr,
        from_local_addrs: Vec<SocketAddr>,
    },
    /// Direct UDP hole-punch probe sent peer-to-peer.
    HolePunchProbe {
        tx_id: u64,
        sender_node_id: NodeId,
    },
    /// Direct UDP hole-punch response confirming bidirectional reachability.
    HolePunchAck {
        tx_id: u64,
        sender_node_id: NodeId,
    },
    /// Blind forwarding of an end-to-end encrypted QUIC datagram through the stateless relay.
    RelayForward {
        src_node_id: NodeId,
        dst_node_id: NodeId,
        #[serde(with = "byte_vec_serde")]
        payload: Vec<u8>,
    },
}

impl SignalingMessage {
    /// Encodes a signaling packet prefixed with `SIGNALING_MAGIC`.
    pub fn encode_datagram(&self) -> Result<Vec<u8>> {
        let body = postcard::to_allocvec(self)?;
        let mut out = Vec::with_capacity(SIGNALING_MAGIC.len() + body.len());
        out.extend_from_slice(&SIGNALING_MAGIC);
        out.extend_from_slice(&body);
        Ok(out)
    }

    /// Returns true if raw UDP bytes carry the Veloce signaling magic header.
    pub fn is_signaling_datagram(bytes: &[u8]) -> bool {
        bytes.len() >= SIGNALING_MAGIC.len() && bytes[..SIGNALING_MAGIC.len()] == SIGNALING_MAGIC
    }

    /// Decodes a signaling packet after verifying `SIGNALING_MAGIC`.
    pub fn decode_datagram(bytes: &[u8]) -> Result<Self> {
        if !Self::is_signaling_datagram(bytes) {
            return Err(ProtocolError::InvalidSignalingPacket(
                "missing Veloce signaling magic".into(),
            ));
        }
        postcard::from_bytes(&bytes[SIGNALING_MAGIC.len()..])
            .map_err(|e| ProtocolError::InvalidSignalingPacket(e.to_string()))
    }
}

/// Serializes and writes a 4-byte length-prefixed postcard frame to an async stream.
pub async fn write_frame<W, T>(writer: &mut W, msg: &T, max_size: usize) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let payload = postcard::to_allocvec(msg)?;
    if payload.len() > max_size {
        return Err(ProtocolError::FrameTooLarge {
            size: payload.len(),
            max: max_size,
        });
    }
    let len_u32 = u32::try_from(payload.len()).map_err(|_| ProtocolError::FrameTooLarge {
        size: payload.len(),
        max: max_size,
    })?;
    writer.write_all(&len_u32.to_be_bytes()).await?;
    writer.write_all(&payload).await?;
    Ok(())
}

/// Reads and deserializes a 4-byte length-prefixed postcard frame from an async stream.
/// Returns `Ok(None)` on clean EOF right at a frame boundary.
pub async fn read_frame<R, T>(reader: &mut R, max_size: usize) -> Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let mut len_buf = [0u8; 4];
    let mut read_so_far = 0;
    while read_so_far < 4 {
        let n = reader.read(&mut len_buf[read_so_far..]).await?;
        if n == 0 {
            if read_so_far == 0 {
                return Ok(None);
            }
            return Err(ProtocolError::UnexpectedEof);
        }
        read_so_far += n;
    }

    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > max_size {
        return Err(ProtocolError::FrameTooLarge {
            size: frame_len,
            max: max_size,
        });
    }

    let mut payload = vec![0u8; frame_len];
    reader.read_exact(&mut payload).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            ProtocolError::UnexpectedEof
        } else {
            ProtocolError::Io(e)
        }
    })?;

    let decoded = postcard::from_bytes(&payload)?;
    Ok(Some(decoded))
}

const DATA_TAG_CHUNK_RESPONSE: u8 = 0x81;
const DATA_TAG_POSTCARD_FALLBACK: u8 = 0x80;

/// Zero-extra-copy streaming writer for `DataMessage` on high-throughput QUIC data streams.
///
/// Writes `ChunkResponse` directly as `[tag][chunk_index][hash][proof_len][proof...][data_len][raw data]`
/// to avoid double-allocating multi-megabyte chunk payloads inside a serialization buffer.
pub async fn write_data_frame<W>(writer: &mut W, msg: &DataMessage, max_size: usize) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    match msg {
        DataMessage::ChunkResponse {
            chunk_index,
            hash,
            proof,
            data,
        } => {
            let header_len = 1 + 4 + 32 + 2 + proof.len() * 32 + 4;
            let total_len = header_len.saturating_add(data.len());
            if total_len > max_size {
                return Err(ProtocolError::FrameTooLarge {
                    size: total_len,
                    max: max_size,
                });
            }
            let proof_len_u16 = u16::try_from(proof.len()).map_err(|_| {
                ProtocolError::FrameTooLarge {
                    size: total_len,
                    max: max_size,
                }
            })?;
            let data_len_u32 = u32::try_from(data.len()).map_err(|_| {
                ProtocolError::FrameTooLarge {
                    size: total_len,
                    max: max_size,
                }
            })?;

            let mut hdr = Vec::with_capacity(header_len);
            hdr.push(DATA_TAG_CHUNK_RESPONSE);
            hdr.extend_from_slice(&chunk_index.to_be_bytes());
            hdr.extend_from_slice(hash);
            hdr.extend_from_slice(&proof_len_u16.to_be_bytes());
            for node in proof {
                hdr.extend_from_slice(node);
            }
            hdr.extend_from_slice(&data_len_u32.to_be_bytes());

            writer.write_all(&hdr).await?;
            writer.write_all(data).await?;
            Ok(())
        }
        other => {
            let payload = postcard::to_allocvec(other)?;
            if payload.len() + 1 > max_size {
                return Err(ProtocolError::FrameTooLarge {
                    size: payload.len() + 1,
                    max: max_size,
                });
            }
            let len_u32 = u32::try_from(payload.len()).map_err(|_| ProtocolError::FrameTooLarge {
                size: payload.len(),
                max: max_size,
            })?;
            let mut hdr = [0u8; 5];
            hdr[0] = DATA_TAG_POSTCARD_FALLBACK;
            hdr[1..5].copy_from_slice(&len_u32.to_be_bytes());
            writer.write_all(&hdr).await?;
            writer.write_all(&payload).await?;
            Ok(())
        }
    }
}

/// Zero-extra-copy streaming reader matching `write_data_frame`.
pub async fn read_data_frame<R>(reader: &mut R, max_size: usize) -> Result<Option<DataMessage>>
where
    R: AsyncRead + Unpin,
{
    let mut tag_buf = [0u8; 1];
    let n = reader.read(&mut tag_buf).await?;
    if n == 0 {
        return Ok(None);
    }

    match tag_buf[0] {
        DATA_TAG_CHUNK_RESPONSE => {
            let mut fixed = [0u8; 38]; // chunk_index (4) + hash (32) + proof_len (2)
            reader.read_exact(&mut fixed).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    ProtocolError::UnexpectedEof
                } else {
                    ProtocolError::Io(e)
                }
            })?;
            let chunk_index = u32::from_be_bytes([fixed[0], fixed[1], fixed[2], fixed[3]]);
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&fixed[4..36]);
            let proof_len = u16::from_be_bytes([fixed[36], fixed[37]]) as usize;

            // Bound Merkle proof depth safely (64 levels covers 2^64 chunks).
            if proof_len > 64 {
                return Err(ProtocolError::FrameTooLarge {
                    size: proof_len * 32,
                    max: 64 * 32,
                });
            }

            let mut proof_bytes = vec![0u8; proof_len * 32];
            if !proof_bytes.is_empty() {
                reader.read_exact(&mut proof_bytes).await.map_err(|e| {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        ProtocolError::UnexpectedEof
                    } else {
                        ProtocolError::Io(e)
                    }
                })?;
            }
            let proof: Vec<[u8; 32]> = proof_bytes.as_chunks::<32>().0.to_vec();

            let mut data_len_buf = [0u8; 4];
            reader.read_exact(&mut data_len_buf).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    ProtocolError::UnexpectedEof
                } else {
                    ProtocolError::Io(e)
                }
            })?;
            let data_len = u32::from_be_bytes(data_len_buf) as usize;
            let total_len = 1 + 38 + proof_len * 32 + 4 + data_len;
            if total_len > max_size {
                return Err(ProtocolError::FrameTooLarge {
                    size: total_len,
                    max: max_size,
                });
            }

            let mut data = vec![0u8; data_len];
            reader.read_exact(&mut data).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    ProtocolError::UnexpectedEof
                } else {
                    ProtocolError::Io(e)
                }
            })?;

            Ok(Some(DataMessage::ChunkResponse {
                chunk_index,
                hash,
                proof,
                data,
            }))
        }
        DATA_TAG_POSTCARD_FALLBACK => {
            let mut len_buf = [0u8; 4];
            reader.read_exact(&mut len_buf).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    ProtocolError::UnexpectedEof
                } else {
                    ProtocolError::Io(e)
                }
            })?;
            let frame_len = u32::from_be_bytes(len_buf) as usize;
            if frame_len > max_size {
                return Err(ProtocolError::FrameTooLarge {
                    size: frame_len,
                    max: max_size,
                });
            }
            let mut payload = vec![0u8; frame_len];
            reader.read_exact(&mut payload).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    ProtocolError::UnexpectedEof
                } else {
                    ProtocolError::Io(e)
                }
            })?;
            Ok(Some(postcard::from_bytes(&payload)?))
        }
        other => Err(ProtocolError::InvalidSignalingPacket(format!(
            "unknown data frame tag 0x{other:02x}"
        ))),
    }
}
