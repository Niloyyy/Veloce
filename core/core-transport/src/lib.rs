pub mod discovery;
pub mod endpoint;
pub mod error;
pub mod identity;
pub mod magic_socket;
pub mod nat;
pub mod transfer;

pub use discovery::{
    classify_interface_priority, local_lan_addrs, DiscoveryEvent, LanDiscovery, LocalPeer,
    MDNS_SERVICE_TYPE,
};
pub use endpoint::{
    classify_socket_addr, EndpointChannels, EndpointOptions, HostedSession, IncomingOffer,
    IncomingOfferDecision, TransferConnectionChanged, VeloceEndpoint,
};
pub use error::{Result, TransportError};
pub use identity::{
    build_bbr_transport_config, extract_node_id_from_cert, peer_node_id, PeerIdentity,
};
pub use magic_socket::{MagicUdpSocket, PathChangeEvent};
pub use nat::{NatConnectPlan, NatCoordinator};
pub use transfer::{
    run_receiver_session, run_sender_session, TransferConfig, TransferDirection, TransferProgress,
    TransferSummary, DEFAULT_PARALLEL_STREAMS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use core_protocol::{ConnectionType, SessionTicket};
    use core_storage::{ChunkAssembler, ChunkSource};
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::atomic::Ordering;
    use tempfile::tempdir;
    use veloce_relay::RelayServer;

    fn loopback_opts(name: &str) -> EndpointOptions {
        EndpointOptions {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            device_name: name.into(),
            enable_mdns: false,
            parallel_streams: 8,
            relay_addrs: Vec::new(),
            force_relay: false,
        }
    }

    #[test]
    fn ed25519_identity_and_cert_extraction() {
        let id = PeerIdentity::generate().unwrap();
        let extracted = extract_node_id_from_cert(&id.cert_der()).unwrap();
        assert_eq!(extracted, id.node_id());
    }

    #[tokio::test]
    async fn quic_bbr_multi_stream_ticket_pull_and_resume() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let bundle = src_dir.path().join("media");
        fs::create_dir_all(&bundle).unwrap();
        let payload_a: Vec<u8> = (0..1_200_000u32).map(|i| ((i * 31) % 251) as u8).collect();
        let payload_b: Vec<u8> = (0..350_000u32).map(|i| ((i * 17) % 241) as u8).collect();
        fs::write(bundle.join("clip.bin"), &payload_a).unwrap();
        fs::write(bundle.join("notes.bin"), &payload_b).unwrap();

        let source = ChunkSource::from_paths_async(vec![bundle], Some(64 * 1024))
            .await
            .unwrap();

        let (sender, _sender_ch) = VeloceEndpoint::bind(loopback_opts("Sender")).await.unwrap();
        let (receiver, mut receiver_ch) = VeloceEndpoint::bind(loopback_opts("Receiver"))
            .await
            .unwrap();

        // Pre-seed half of the chunks to exercise instant resume over QUIC.
        {
            let pre_assembler =
                ChunkAssembler::open(dst_dir.path(), source.manifest().clone()).unwrap();
            for idx in (0..source.manifest().total_chunks).step_by(2) {
                let chunk = source.read_chunk(idx).unwrap();
                pre_assembler
                    .write_verified_chunk(chunk.chunk_index, &chunk.data, &chunk.proof)
                    .unwrap();
            }
            pre_assembler.persist_state().unwrap();
        }

        let hosted = sender
            .host_session(source.clone(), vec![sender.listen_addr()], vec![])
            .unwrap();
        let decoded_ticket = SessionTicket::decode(&hosted.ticket_code).unwrap();

        let summary = receiver
            .pull_session_direct(
                &decoded_ticket,
                sender.listen_addr(),
                dst_dir.path().to_path_buf(),
                8,
                ConnectionType::Lan,
            )
            .await
            .unwrap();

        assert_eq!(summary.total_chunks, source.manifest().total_chunks);
        assert_eq!(summary.finalized_paths.len(), 2);

        let mut saw_progress = false;
        while let Ok(p) = receiver_ch.progress_rx.try_recv() {
            if p.chunks_verified == p.total_chunks {
                saw_progress = true;
            }
        }
        assert!(saw_progress);

        assert_eq!(
            fs::read(dst_dir.path().join("media/clip.bin")).unwrap(),
            payload_a
        );
        assert_eq!(
            fs::read(dst_dir.path().join("media/notes.bin")).unwrap(),
            payload_b
        );

        sender.shutdown();
        receiver.shutdown();
    }

    #[tokio::test]
    async fn quic_lan_push_approval_and_rejection_flows() {
        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();

        let file_path = src_dir.path().join("hello.dat");
        let data: Vec<u8> = (0..400_000u32).map(|i| (i % 256) as u8).collect();
        fs::write(&file_path, &data).unwrap();
        let source = ChunkSource::from_paths_async(vec![file_path], Some(64 * 1024))
            .await
            .unwrap();

        let (sender, _s_ch) = VeloceEndpoint::bind(loopback_opts("Laptop")).await.unwrap();
        let (receiver, mut r_ch) = VeloceEndpoint::bind(loopback_opts("Desktop")).await.unwrap();

        let reject_task = tokio::spawn(async move {
            let offer = r_ch.incoming_offers.recv().await.unwrap();
            assert_eq!(offer.sender_name, "Laptop");
            let _ = offer.responder.send(IncomingOfferDecision::Reject {
                reason: "busy".into(),
            });
            r_ch
        });

        let err = sender
            .push_to_peer(
                Some(receiver.node_id()),
                receiver.listen_addr(),
                source.clone(),
                ConnectionType::Lan,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, TransportError::Rejected(_)));

        let mut r_ch = reject_task.await.unwrap();

        let out_path = dst_dir.path().to_path_buf();
        let accept_task = tokio::spawn(async move {
            let offer = r_ch.incoming_offers.recv().await.unwrap();
            let _ = offer.responder.send(IncomingOfferDecision::Accept {
                output_dir: out_path,
            });
        });

        let summary = sender
            .push_to_peer(
                Some(receiver.node_id()),
                receiver.listen_addr(),
                source.clone(),
                ConnectionType::Lan,
            )
            .await
            .unwrap();
        accept_task.await.unwrap();

        assert_eq!(summary.total_bytes, data.len() as u64);
        assert_eq!(fs::read(dst_dir.path().join("hello.dat")).unwrap(), data);

        sender.shutdown();
        receiver.shutdown();
    }

    #[tokio::test]
    async fn mtls_rejects_wrong_expected_node_id() {
        let (server, _s_ch) = VeloceEndpoint::bind(loopback_opts("Server")).await.unwrap();
        let (client, _c_ch) = VeloceEndpoint::bind(loopback_opts("Client")).await.unwrap();
        let bogus_id = core_protocol::NodeId::from_bytes([0xFF; 32]);

        let src_dir = tempdir().unwrap();
        let f = src_dir.path().join("x.bin");
        fs::write(&f, b"secret").unwrap();
        let source = ChunkSource::from_paths_async(vec![f], None).await.unwrap();

        let err = client
            .push_to_peer(
                Some(bogus_id),
                server.listen_addr(),
                source,
                ConnectionType::Lan,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, TransportError::Connection(_)));

        server.shutdown();
        client.shutdown();
    }

    #[tokio::test]
    async fn global_session_udp_hole_punching_to_direct_p2p() {
        let relay = RelayServer::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .unwrap();
        let relay_addr = relay.local_addr();

        let mut s_opts = loopback_opts("GlobalSender");
        s_opts.relay_addrs = vec![relay_addr];
        let mut r_opts = loopback_opts("GlobalReceiver");
        r_opts.relay_addrs = vec![relay_addr];

        let (sender, _s_ch) = VeloceEndpoint::bind(s_opts).await.unwrap();
        let (receiver, mut r_ch) = VeloceEndpoint::bind(r_opts).await.unwrap();

        // STUN discovery succeeds against relay.
        let discovered = sender.nat().discover_public_addr(relay_addr).await.unwrap();
        assert_eq!(discovered, sender.listen_addr());

        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();
        let data: Vec<u8> = (0..500_000u32).map(|i| ((i * 19) % 253) as u8).collect();
        let file_path = src_dir.path().join("global_punch.bin");
        fs::write(&file_path, &data).unwrap();
        let source = ChunkSource::from_paths_async(vec![file_path], Some(64 * 1024))
            .await
            .unwrap();

        // Build a ticket with NO direct_addrs so join_session MUST discover & UDP hole-punch via relay!
        let hosted = sender
            .host_session(source.clone(), vec![], vec![relay_addr.to_string()])
            .unwrap();
        let mut ticket = SessionTicket::decode(&hosted.ticket_code).unwrap();
        ticket.direct_addrs.clear();

        let summary = receiver
            .join_session(&ticket, dst_dir.path().to_path_buf(), Some(6))
            .await
            .unwrap();

        assert_eq!(summary.total_bytes, data.len() as u64);
        assert_eq!(
            fs::read(dst_dir.path().join("global_punch.bin")).unwrap(),
            data
        );

        // Verify Direct-P2P path was established via hole-punching (0 relayed data packets!).
        let conn_evt = r_ch.conn_changed_rx.try_recv().unwrap();
        assert_eq!(conn_evt.connection_type, ConnectionType::DirectP2p);
        assert_eq!(relay.stats().forwarded_packets.load(Ordering::Relaxed), 0);

        sender.shutdown();
        receiver.shutdown();
        relay.shutdown();
    }

    #[tokio::test]
    async fn global_session_encrypted_relay_fallback_and_live_upgrade() {
        let relay = RelayServer::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .unwrap();
        let relay_addr = relay.local_addr();

        // Force both endpoints through the blind encrypted relay initially (simulating symmetric NATs).
        let mut s_opts = loopback_opts("SymmetricSender");
        s_opts.relay_addrs = vec![relay_addr];
        s_opts.force_relay = true;

        let mut r_opts = loopback_opts("SymmetricReceiver");
        r_opts.relay_addrs = vec![relay_addr];
        r_opts.force_relay = true;

        let (sender, _s_ch) = VeloceEndpoint::bind(s_opts).await.unwrap();
        let (receiver, mut r_ch) = VeloceEndpoint::bind(r_opts).await.unwrap();

        let src_dir = tempdir().unwrap();
        let dst_dir = tempdir().unwrap();
        let data: Vec<u8> = (0..600_000u32).map(|i| ((i * 7) % 251) as u8).collect();
        let file_path = src_dir.path().join("relayed_e2ee.bin");
        fs::write(&file_path, &data).unwrap();
        let source = ChunkSource::from_paths_async(vec![file_path], Some(64 * 1024))
            .await
            .unwrap();

        let hosted = sender
            .host_session(source.clone(), vec![], vec![relay_addr.to_string()])
            .unwrap();
        let mut ticket = SessionTicket::decode(&hosted.ticket_code).unwrap();
        ticket.direct_addrs.clear();

        let summary = receiver
            .join_session(&ticket, dst_dir.path().to_path_buf(), Some(4))
            .await
            .unwrap();

        assert_eq!(summary.total_bytes, data.len() as u64);
        assert_eq!(
            fs::read(dst_dir.path().join("relayed_e2ee.bin")).unwrap(),
            data
        );

        // Verify relay forwarded encrypted QUIC packets and reported ConnectionType::Relayed!
        let conn_evt = r_ch.conn_changed_rx.try_recv().unwrap();
        assert_eq!(conn_evt.connection_type, ConnectionType::Relayed);
        assert!(relay.stats().forwarded_packets.load(Ordering::Relaxed) > 0);

        // Now verify live upgrade from Relayed -> DirectP2p when direct UDP opens up!
        sender.set_force_relay(false);
        receiver.set_force_relay(false);
        let mut path_rx = receiver.subscribe_path_changes();
        let punched = receiver
            .nat()
            .attempt_hole_punch(
                sender.node_id(),
                &[sender.listen_addr()],
                std::time::Duration::from_secs(2),
            )
            .await;
        assert_eq!(punched, Some(sender.listen_addr()));
        let change = path_rx.try_recv().unwrap();
        assert_eq!(change.connection_type, ConnectionType::DirectP2p);

        sender.shutdown();
        receiver.shutdown();
        relay.shutdown();
    }

    #[test]
    fn lan_interface_classifier_filters_virtual_bridges_and_ranks_physical_nics() {
        let lan_v4 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 25));
        let docker_v4 = IpAddr::V4(Ipv4Addr::new(172, 17, 0, 1));
        let link_local_v4 = IpAddr::V4(Ipv4Addr::new(169, 254, 12, 3));
        let vpn_v4 = IpAddr::V4(Ipv4Addr::new(100, 64, 0, 5));
        let lo_v4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let ll_v6: IpAddr = "fe80::1".parse().unwrap();
        let ula_v6: IpAddr = "fd00::42".parse().unwrap();

        assert_eq!(classify_interface_priority("enp3s0", lan_v4), Some(0));
        assert_eq!(classify_interface_priority("wlan0", lan_v4), Some(0));
        assert_eq!(classify_interface_priority("en0", ula_v6), Some(2));
        assert_eq!(classify_interface_priority("tailscale0", vpn_v4), Some(3));
        assert_eq!(classify_interface_priority("wg0", vpn_v4), Some(3));
        assert_eq!(classify_interface_priority("tun0", vpn_v4), Some(3));

        // Excluded interfaces & non-routable addresses.
        assert_eq!(classify_interface_priority("docker0", docker_v4), None);
        assert_eq!(classify_interface_priority("br-a1b2c3d4", docker_v4), None);
        assert_eq!(classify_interface_priority("veth12345", docker_v4), None);
        assert_eq!(classify_interface_priority("virbr0", lan_v4), None);
        assert_eq!(classify_interface_priority("vmnet8", lan_v4), None);
        assert_eq!(classify_interface_priority("vboxnet0", lan_v4), None);
        assert_eq!(classify_interface_priority("lo", lo_v4), None);
        assert_eq!(classify_interface_priority("eth0", link_local_v4), None);
        assert_eq!(classify_interface_priority("eth0", ll_v6), None);
    }

    #[tokio::test]
    async fn magic_socket_survives_malformed_udp_bombardment_without_panic() {
        use core_protocol::{NodeId, SignalingMessage, SIGNALING_MAGIC};
        use quinn::udp::RecvMeta;
        use quinn::AsyncUdpSocket;
        use std::io::IoSliceMut;
        use std::task::{Context, Poll};

        let local_id = NodeId([0x55; 32]);
        let (magic, mut sig_rx) = MagicUdpSocket::bind(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            local_id,
        )
        .await
        .unwrap();
        let target_addr = magic.local_addr().unwrap();

        // Verify socket2 buffer tuning succeeded without failing bind.
        let (rcv, snd) = magic.configured_buffer_sizes();
        assert!(rcv > 0 && snd > 0);

        let attacker = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();

        // 1. Send adversarial/malformed packets covering every demux branch.
        let oversized_relay = SignalingMessage::RelayForward {
            src_node_id: NodeId([0x99; 32]),
            dst_node_id: local_id,
            payload: vec![0xC0; 3000], // exceeds MAX_RELAY_FORWARD_PAYLOAD (2048)
        }
        .encode_datagram()
        .unwrap();
        let bad_packets: Vec<Vec<u8>> = vec![
            vec![],
            vec![0x1B],
            SIGNALING_MAGIC.to_vec(),
            [SIGNALING_MAGIC.as_slice(), &[0xFF; 64]].concat(),
            [SIGNALING_MAGIC.as_slice(), &[9, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F]].concat(),
            oversized_relay,
        ];
        for pkt in bad_packets {
            let _ = attacker.send_to(&pkt, target_addr).await;
        }

        // 2. Follow up with a valid signaling STUN packet and a valid raw QUIC-like packet.
        let valid_stun = SignalingMessage::StunRequest { tx_id: 777 }
            .encode_datagram()
            .unwrap();
        attacker.send_to(&valid_stun, target_addr).await.unwrap();

        let valid_quic_like = vec![0xC0, 0x00, 0x00, 0x01, 0xAA, 0xBB, 0xCC, 0xDD];
        attacker
            .send_to(&valid_quic_like, target_addr)
            .await
            .unwrap();

        // Give loopback UDP a brief moment to queue all packets.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Drain poll_recv until the valid QUIC-like packet surfaces (accounting for recvmmsg batching),
        // confirming empty, malformed `0x1BVLC`, and oversized RelayForward packets are dropped safely.
        let mut received_packets: Vec<Vec<u8>> = Vec::new();
        let deadline = tokio::time::sleep(std::time::Duration::from_secs(2));
        tokio::pin!(deadline);

        while !received_packets.iter().any(|p| p == &valid_quic_like) {
            let mut b0 = vec![0u8; 2048];
            let mut b1 = vec![0u8; 2048];
            let mut slices = [IoSliceMut::new(&mut b0), IoSliceMut::new(&mut b1)];
            let mut meta = [RecvMeta::default(); 2];

            let count = tokio::select! {
                _ = &mut deadline => panic!("timed out waiting for valid QUIC packet after malformed bombardment"),
                res = std::future::poll_fn(|cx: &mut Context<'_>| {
                    match magic.poll_recv(cx, &mut slices, &mut meta) {
                        Poll::Ready(res) => Poll::Ready(res),
                        Poll::Pending => Poll::Pending,
                    }
                }) => res.unwrap(),
            };
            for (i, item) in meta.iter().enumerate().take(count) {
                let len = item.len;
                let data = if i == 0 {
                    b0[..len].to_vec()
                } else {
                    b1[..len].to_vec()
                };
                received_packets.push(data);
            }
        }

        // Neither empty nor any `0x1BVLC` malformed/oversized packet may leak to QUIC.
        assert!(received_packets.iter().all(|p| !p.is_empty() && !SignalingMessage::is_signaling_datagram(p)));
        assert!(received_packets.contains(&valid_quic_like));

        // The valid STUN signaling packet must also have been cleanly demultiplexed.
        let (_from, msg) = tokio::time::timeout(std::time::Duration::from_secs(1), sig_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(msg, SignalingMessage::StunRequest { tx_id: 777 });
    }
}
