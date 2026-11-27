pub mod error;
pub mod handshake;
pub mod messages;
pub mod ticket;
pub mod types;

pub use error::{ProtocolError, Result};
pub use handshake::{HandshakeState, HandshakeStateMachine, SessionRole};
pub use messages::{
    encode_chunk_response_header, encode_postcard_data_frame, read_data_frame, read_frame,
    write_data_frame, write_frame, ChunkErrorCode, ControlMessage, DataMessage, SignalingMessage,
    ZeroCopyDataFrame, DATA_TAG_CHUNK_RESPONSE, DATA_TAG_POSTCARD_FALLBACK, MAX_CONTROL_FRAME_SIZE,
    MAX_DATA_FRAME_SIZE, SIGNALING_MAGIC,
};
pub use ticket::{SessionTicket, TICKET_PREFIX};
pub use types::{
    validate_relative_path, ConnectionType, FileEntry, NodeId, TransferManifest, TransferProfile,
    ALPN_VELOCE, PROTOCOL_VERSION,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn sample_manifest() -> TransferManifest {
        let h0 = *blake3::hash(b"chunk0").as_bytes();
        let h1 = *blake3::hash(b"chunk1").as_bytes();
        TransferManifest {
            session_id: [7u8; 16],
            files: vec![
                FileEntry {
                    relative_path: "docs/report.pdf".into(),
                    size: 1500,
                    first_chunk: 0,
                    chunk_count: 2,
                    file_hash: *blake3::hash(b"chunk0chunk1").as_bytes(),
                    mode: Some(0o644),
                },
                FileEntry {
                    relative_path: "docs/empty.txt".into(),
                    size: 0,
                    first_chunk: 2,
                    chunk_count: 0,
                    file_hash: *blake3::hash(b"").as_bytes(),
                    mode: Some(0o644),
                },
            ],
            chunk_size: 1024,
            total_size: 1500,
            total_chunks: 2,
            chunk_hashes: vec![h0, h1],
            merkle_root: [0xAB; 32],
        }
    }

    #[test]
    fn node_id_base32_and_hex_roundtrip() {
        let id = NodeId::from_bytes([0x42; 32]);
        let b32 = id.to_base32();
        assert_eq!(NodeId::parse(&b32).unwrap(), id);
        let hex_str = hex::encode(id.as_bytes());
        assert_eq!(NodeId::parse(&hex_str).unwrap(), id);
        assert_eq!(id.short_id().len(), 8);
    }

    #[test]
    fn session_ticket_roundtrip_and_tamper_detection() {
        let ticket = SessionTicket::new(
            NodeId::from_bytes([0x11; 32]),
            vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42)), 5150)],
            vec!["relay.veloce.dev:7878".into()],
            [0xEE; 32],
            [0x99; 16],
            4_200_000,
            3,
        );
        let encoded = ticket.encode().unwrap();
        assert!(encoded.starts_with(TICKET_PREFIX));

        let decoded = SessionTicket::decode(&encoded).unwrap();
        assert_eq!(decoded, ticket);

        // URI scheme and whitespace tolerance.
        let uri_wrapped = format!("veloce://{encoded}");
        assert_eq!(SessionTicket::decode(&uri_wrapped).unwrap(), ticket);

        // Flip a character in the payload — checksum must reject it.
        let mut tampered = encoded.into_bytes();
        let idx = TICKET_PREFIX.len() + 2;
        tampered[idx] = if tampered[idx] == b'a' { b'b' } else { b'a' };
        let tampered_str = String::from_utf8(tampered).unwrap();
        assert!(SessionTicket::decode(&tampered_str).is_err());
    }

    #[test]
    fn path_validation_blocks_traversal_and_unsafe_paths() {
        assert!(validate_relative_path("a/b/c.txt").is_ok());
        assert!(validate_relative_path("../secret").is_err());
        assert!(validate_relative_path("a/../../secret").is_err());
        assert!(validate_relative_path("/etc/passwd").is_err());
        assert!(validate_relative_path("C:foo.txt").is_err());
        assert!(validate_relative_path("dir\\file.txt").is_err());
        assert!(validate_relative_path("bad\0name").is_err());
        assert!(validate_relative_path("").is_err());
    }

    #[test]
    fn manifest_locate_chunk_and_validation() {
        let manifest = sample_manifest();
        manifest.validate().unwrap();
        assert_eq!(manifest.locate_chunk(0), Some((0, 0, 1024)));
        assert_eq!(manifest.locate_chunk(1), Some((0, 1024, 476)));
        assert_eq!(manifest.locate_chunk(2), None);
    }

    #[test]
    fn handshake_state_machine_full_lifecycle() {
        let token = [0x5A; 16];
        let mut sender = HandshakeStateMachine::new(SessionRole::Sender, Some(token));
        let mut receiver = HandshakeStateMachine::new(SessionRole::Receiver, Some(token));

        let sender_hello = ControlMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            node_id: NodeId([1; 32]),
            device_name: "Sender-Linux".into(),
            session_token: Some(token),
        };
        let receiver_hello = ControlMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            node_id: NodeId([2; 32]),
            device_name: "Receiver-Mac".into(),
            session_token: Some(token),
        };

        sender.on_send(&sender_hello).unwrap();
        receiver.on_receive(sender_hello).unwrap();
        receiver.on_send(&receiver_hello).unwrap();
        sender.on_receive(receiver_hello).unwrap();

        let offer = ControlMessage::Offer {
            manifest: sample_manifest(),
            sender_name: "Sender-Linux".into(),
        };
        sender.on_send(&offer).unwrap();
        receiver.on_receive(offer).unwrap();

        let accept = ControlMessage::Accept {
            resume_bitfield: vec![0b0000_0001],
        };
        receiver.on_send(&accept).unwrap();
        sender.on_receive(accept).unwrap();

        let complete = ControlMessage::TransferComplete { verified_chunks: 2 };
        receiver.on_send(&complete).unwrap();
        sender.on_receive(complete).unwrap();

        assert!(matches!(
            sender.state(),
            HandshakeState::Completed { verified_chunks: 2 }
        ));
        assert!(matches!(
            receiver.state(),
            HandshakeState::Completed { verified_chunks: 2 }
        ));
    }

    #[test]
    fn handshake_rejects_bad_token_and_bad_version() {
        let mut sender = HandshakeStateMachine::new(SessionRole::Sender, Some([1; 16]));
        let bad_token = ControlMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            node_id: NodeId([2; 32]),
            device_name: "Intruder".into(),
            session_token: Some([9; 16]),
        };
        assert!(matches!(
            sender.on_receive(bad_token),
            Err(ProtocolError::SessionTokenMismatch)
        ));

        let bad_version = ControlMessage::Hello {
            protocol_version: 99,
            node_id: NodeId([2; 32]),
            device_name: "Future".into(),
            session_token: Some([1; 16]),
        };
        assert!(matches!(
            sender.on_receive(bad_version),
            Err(ProtocolError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn signaling_magic_never_collides_with_quic_fixed_bit() {
        // RFC 9000 requires fixed bit (0x40) == 1 on QUIC packets.
        assert_eq!(SIGNALING_MAGIC[0] & 0x40, 0);

        let msg = SignalingMessage::RelayForward {
            src_node_id: NodeId([3; 32]),
            dst_node_id: NodeId([4; 32]),
            payload: vec![0xC0, 0x00, 0x00, 0x01],
        };
        let encoded = msg.encode_datagram().unwrap();
        assert!(SignalingMessage::is_signaling_datagram(&encoded));
        assert_eq!(SignalingMessage::decode_datagram(&encoded).unwrap(), msg);
    }

    #[tokio::test]
    async fn framed_stream_roundtrip_and_size_limit() {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let msg = DataMessage::ChunkResponse {
            chunk_index: 5,
            hash: [0x33; 32],
            proof: vec![[0x44; 32], [0x55; 32]],
            data: vec![0xAB; 512],
        };
        write_frame(&mut client, &msg, MAX_DATA_FRAME_SIZE)
            .await
            .unwrap();
        drop(client);

        let received: Option<DataMessage> =
            read_frame(&mut server, MAX_DATA_FRAME_SIZE).await.unwrap();
        assert_eq!(received, Some(msg));
        let eof: Option<DataMessage> = read_frame(&mut server, MAX_DATA_FRAME_SIZE).await.unwrap();
        assert_eq!(eof, None);
    }

    #[test]
    fn adversarial_ticket_and_signaling_inputs_never_panic() {
        let huge = format!("veloce1{}", "a".repeat(128 * 1024));
        let bad_tickets = [
            "",
            "   ",
            "veloce1",
            "veloce://",
            "veloce://veloce1",
            "veloce1!!!!",
            "veloce1aaaa",
            "veloce1aaaaaaaaaaaa",
            "not-a-ticket\0with-nul",
            "veloce1\u{1F680}",
            &huge,
        ];
        for sample in bad_tickets {
            assert!(
                SessionTicket::decode(sample).is_err(),
                "expected error for malformed ticket len={}",
                sample.len()
            );
        }

        // Valid checksum wrapping malformed postcard payload must still error cleanly.
        let garbage = vec![0xFFu8; 32];
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"VELOCE-TICKET-V1");
        hasher.update(&garbage);
        let digest = hasher.finalize();
        let tag = &digest.as_bytes()[..4];
        let mut raw = garbage;
        raw.extend_from_slice(tag);
        let crafted = format!(
            "{}{}",
            TICKET_PREFIX,
            data_encoding::BASE32_NOPAD.encode(&raw).to_ascii_lowercase()
        );
        assert!(SessionTicket::decode(&crafted).is_err());

        // Adversarial signaling UDP datagrams.
        let bad_datagrams: Vec<Vec<u8>> = vec![
            vec![],
            vec![0x1B],
            vec![0x1B, b'V', b'L'],
            SIGNALING_MAGIC.to_vec(),
            [SIGNALING_MAGIC.as_slice(), &[0xFF; 16]].concat(),
            [SIGNALING_MAGIC.as_slice(), &[9, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F]].concat(), // huge varint length
            vec![0xC0, 0x00, 0x00, 0x01, 0x08],                                        // QUIC long header
        ];
        for pkt in bad_datagrams {
            assert!(SignalingMessage::decode_datagram(&pkt).is_err());
        }
    }

    #[tokio::test]
    async fn adversarial_frame_lengths_and_truncated_streams_rejected() {
        use tokio::io::AsyncWriteExt;

        // 1. Oversized u32::MAX frame header rejected immediately without allocating.
        let (mut client, mut server) = tokio::io::duplex(64);
        client.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
        drop(client);
        let res: Result<Option<ControlMessage>> =
            read_frame(&mut server, MAX_CONTROL_FRAME_SIZE).await;
        assert!(matches!(res, Err(ProtocolError::FrameTooLarge { .. })));

        // 2. Truncated length header (2 bytes then EOF).
        let (mut client, mut server) = tokio::io::duplex(64);
        client.write_all(&[0x00, 0x01]).await.unwrap();
        drop(client);
        let res: Result<Option<ControlMessage>> =
            read_frame(&mut server, MAX_CONTROL_FRAME_SIZE).await;
        assert!(matches!(res, Err(ProtocolError::UnexpectedEof)));

        // 3. Truncated body (claims 512 bytes, sends 8 bytes then EOF).
        let (mut client, mut server) = tokio::io::duplex(64);
        client.write_all(&512u32.to_be_bytes()).await.unwrap();
        client.write_all(&[0xAA; 8]).await.unwrap();
        drop(client);
        let res: Result<Option<DataMessage>> = read_frame(&mut server, MAX_DATA_FRAME_SIZE).await;
        assert!(matches!(res, Err(ProtocolError::UnexpectedEof)));

        // 4. Malformed postcard claiming massive Vec length inside valid frame size.
        let (mut client, mut server) = tokio::io::duplex(64);
        let crafted = [2u8, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
        client
            .write_all(&(crafted.len() as u32).to_be_bytes())
            .await
            .unwrap();
        client.write_all(&crafted).await.unwrap();
        drop(client);
        let res: Result<Option<DataMessage>> = read_frame(&mut server, MAX_DATA_FRAME_SIZE).await;
        assert!(matches!(res, Err(ProtocolError::Serialization(_))));
    }
}
