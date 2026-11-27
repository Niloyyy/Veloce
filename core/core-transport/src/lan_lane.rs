use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Buf, Bytes, BytesMut};
use core_protocol::{
    DataMessage, NodeId, ProtocolError, TransferProfile, ZeroCopyDataFrame,
    DATA_TAG_CHUNK_RESPONSE, DATA_TAG_POSTCARD_FALLBACK,
};
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, Connection as TlsConnection, ServerConfig as TlsServerConfig, ServerConnection};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::identity::{extract_node_id_from_cert, PeerIdentity};

const LAN_LANE_WIRE_VERSION: u8 = 1;
const BATCH_CIPHERTEXT_FLUSH: usize = 256 * 1024;
const NET_READ_BUF_SIZE: usize = 128 * 1024;
const DECRYPT_SCRATCH_SIZE: usize = 64 * 1024;

type SessionKey = (NodeId, [u8; 16]);

/// High-throughput kernel-offloaded (TSO/GRO) TLS 1.3 TCP data lane bound on the exact same
/// port number as `MagicUdpSocket` for direct `ConnectionType::Lan` transfers.
///
/// Authenticates every stream with mutual Ed25519 TLS 1.3 (`NodeId == Ed25519 public key`)
/// plus a 128-bit `session_id` capability header, falling back transparently to QUIC data
/// streams whenever TCP is unreachable or filtered.
pub struct LanTcpLane {
    local_port: u16,
    sessions: Arc<Mutex<HashMap<SessionKey, mpsc::Sender<LanTlsStream>>>>,
    cancel: CancellationToken,
}

/// RAII guard unregistering a `(peer_node_id, session_id)` route when a sender session ends.
pub struct LanSessionRegistration {
    sessions: Arc<Mutex<HashMap<SessionKey, mpsc::Sender<LanTlsStream>>>>,
    key: SessionKey,
    pub stream_rx: mpsc::Receiver<LanTlsStream>,
}

impl Drop for LanSessionRegistration {
    fn drop(&mut self) {
        self.sessions
            .lock()
            .expect("lan_lane sessions lock poisoned")
            .remove(&self.key);
    }
}

impl LanTcpLane {
    /// Attempts to bind a tuned dual-stack/IPv4 TCP listener on `target_bind` (matching the UDP port).
    /// Returns `None` gracefully if the TCP port is already occupied by another process.
    pub fn try_bind(
        bind_template: SocketAddr,
        port: u16,
        profile: TransferProfile,
        identity: &PeerIdentity,
        parent_cancel: &CancellationToken,
    ) -> Option<Arc<Self>> {
        let tls_server_cfg = identity.build_lan_tls_server_config().ok()?;
        let target = SocketAddr::new(bind_template.ip(), port);
        let std_listener = bind_tuned_tcp_listener(target, profile.udp_buffer_ladder()).ok()?;
        let local_port = std_listener.local_addr().ok()?.port();
        let listener = TcpListener::from_std(std_listener).ok()?;

        let sessions: Arc<Mutex<HashMap<SessionKey, mpsc::Sender<LanTlsStream>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let cancel = parent_cancel.child_token();

        let lane = Arc::new(Self {
            local_port,
            sessions: Arc::clone(&sessions),
            cancel: cancel.clone(),
        });

        let buffer_ladder = profile.udp_buffer_ladder();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    accepted = listener.accept() => {
                        let Ok((tcp, _peer_addr)) = accepted else {
                            continue;
                        };
                        let _ = tcp.set_nodelay(true);
                        tune_accepted_tcp_buffers(&tcp, buffer_ladder);
                        let cfg = Arc::clone(&tls_server_cfg);
                        let reg_map = Arc::clone(&sessions);
                        tokio::spawn(async move {
                            let _ = tokio::time::timeout(
                                Duration::from_secs(5),
                                accept_authenticated_lan_stream(tcp, cfg, reg_map),
                            )
                            .await;
                        });
                    }
                }
            }
        });

        Some(lane)
    }

    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// Registers an active sender session so authenticated incoming LAN TLS streams matching
    /// `(peer_node_id, session_id)` are routed directly to `run_sender_session`.
    pub fn register_session(
        &self,
        peer_node_id: NodeId,
        session_id: [u8; 16],
    ) -> LanSessionRegistration {
        let (tx, rx) = mpsc::channel(16);
        let key = (peer_node_id, session_id);
        self.sessions
            .lock()
            .expect("lan_lane sessions lock poisoned")
            .insert(key, tx);
        LanSessionRegistration {
            sessions: Arc::clone(&self.sessions),
            key,
            stream_rx: rx,
        }
    }

    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

async fn accept_authenticated_lan_stream(
    mut tcp: TcpStream,
    tls_server_cfg: Arc<TlsServerConfig>,
    sessions: Arc<Mutex<HashMap<SessionKey, mpsc::Sender<LanTlsStream>>>>,
) -> io::Result<()> {
    let server_conn = ServerConnection::new(tls_server_cfg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let mut tls = TlsConnection::Server(server_conn);
    complete_tls_handshake(&mut tcp, &mut tls).await?;

    let authed_peer = {
        let certs = tls.peer_certificates().ok_or_else(|| {
            io::Error::new(io::ErrorKind::PermissionDenied, "missing client cert")
        })?;
        let leaf = certs.first().ok_or_else(|| {
            io::Error::new(io::ErrorKind::PermissionDenied, "empty client cert chain")
        })?;
        extract_node_id_from_cert(leaf)
            .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e.to_string()))?
    };

    let mut stream = LanTlsStream::new(tcp, tls);
    // Read `[version: 1B][session_id: 16B]` inside the mutually authenticated TLS 1.3 tunnel.
    if !stream
        .ensure_bytes(17)
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
    {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "missing LAN lane header",
        ));
    }
    let ver = stream.plain_buf.get_u8();
    if ver != LAN_LANE_WIRE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported LAN lane version",
        ));
    }
    let mut session_id = [0u8; 16];
    stream.plain_buf.copy_to_slice(&mut session_id);

    let sender_tx = sessions
        .lock()
        .expect("lan_lane sessions lock poisoned")
        .get(&(authed_peer, session_id))
        .cloned();

    if let Some(tx) = sender_tx {
        let _ = tx.send(stream).await;
    }
    Ok(())
}

/// Dials an authenticated Ed25519 mutual-TLS 1.3 TCP data stream to `target_addr` and binds
/// it to `session_id`.
pub async fn connect_lan_tls_stream(
    target_addr: SocketAddr,
    expected_peer: NodeId,
    session_id: [u8; 16],
    identity: &PeerIdentity,
    profile: TransferProfile,
    connect_timeout: Duration,
) -> io::Result<LanTlsStream> {
    let client_cfg = identity
        .build_lan_tls_client_config(expected_peer)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    let sni_str = format!("{}.veloce.local", expected_peer.to_base32());
    let server_name = ServerName::try_from(sni_str)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;

    let mut tcp = tokio::time::timeout(
        connect_timeout,
        connect_tuned_tcp_stream(target_addr, profile.udp_buffer_ladder()),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "LAN TCP connect timeout"))??;
    let _ = tcp.set_nodelay(true);

    let client_conn = ClientConnection::new(client_cfg, server_name)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let mut tls = TlsConnection::Client(client_conn);
    tokio::time::timeout(Duration::from_secs(3), complete_tls_handshake(&mut tcp, &mut tls))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "LAN TLS handshake timeout"))??;

    let mut stream = LanTlsStream::new(tcp, tls);
    let mut bind_hdr = [0u8; 17];
    bind_hdr[0] = LAN_LANE_WIRE_VERSION;
    bind_hdr[1..17].copy_from_slice(&session_id);
    stream.write_slices(&[&bind_hdr]).await?;
    Ok(stream)
}

async fn complete_tls_handshake(tcp: &mut TcpStream, tls: &mut TlsConnection) -> io::Result<()> {
    let mut read_buf = vec![0u8; 16 * 1024];
    let mut write_buf = Vec::with_capacity(16 * 1024);

    while tls.is_handshaking() {
        while tls.wants_write() {
            write_buf.clear();
            tls.write_tls(&mut write_buf)?;
            if !write_buf.is_empty() {
                tcp.write_all(&write_buf).await?;
            }
        }
        if !tls.is_handshaking() {
            break;
        }
        if tls.wants_read() {
            let n = tcp.read(&mut read_buf).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "EOF during LAN TLS 1.3 handshake",
                ));
            }
            let mut cursor = io::Cursor::new(&read_buf[..n]);
            while (cursor.position() as usize) < n {
                tls.read_tls(&mut cursor)?;
                tls.process_new_packets()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            }
        }
    }

    while tls.wants_write() {
        write_buf.clear();
        tls.write_tls(&mut write_buf)?;
        if !write_buf.is_empty() {
            tcp.write_all(&write_buf).await?;
        }
    }
    Ok(())
}

/// High-throughput TLS 1.3 stream batching 16 KiB `AES-128-GCM` records into 256 KiB kernel TSO writes.
pub struct LanTlsStream {
    tcp: TcpStream,
    tls: TlsConnection,
    net_read_buf: Box<[u8]>,
    decrypt_scratch: Box<[u8]>,
    write_buf: Vec<u8>,
    plain_buf: BytesMut,
    eof: bool,
}

impl LanTlsStream {
    fn new(tcp: TcpStream, mut tls: TlsConnection) -> Self {
        tls.set_buffer_limit(Some(512 * 1024));
        Self {
            tcp,
            tls,
            net_read_buf: vec![0u8; NET_READ_BUF_SIZE].into_boxed_slice(),
            decrypt_scratch: vec![0u8; DECRYPT_SCRATCH_SIZE].into_boxed_slice(),
            write_buf: Vec::with_capacity(BATCH_CIPHERTEXT_FLUSH + 32 * 1024),
            plain_buf: BytesMut::with_capacity(128 * 1024),
            eof: false,
        }
    }

    pub async fn write_slices(&mut self, slices: &[&[u8]]) -> io::Result<()> {
        for &slice in slices {
            let mut offset = 0usize;
            while offset < slice.len() {
                let end = (offset + 64 * 1024).min(slice.len());
                self.tls.writer().write_all(&slice[offset..end])?;
                offset = end;

                while self.tls.wants_write() {
                    self.tls.write_tls(&mut self.write_buf)?;
                    if self.write_buf.len() >= BATCH_CIPHERTEXT_FLUSH {
                        self.tcp.write_all(&self.write_buf).await?;
                        self.write_buf.clear();
                    }
                }
            }
        }
        while self.tls.wants_write() {
            self.tls.write_tls(&mut self.write_buf)?;
        }
        if !self.write_buf.is_empty() {
            self.tcp.write_all(&self.write_buf).await?;
            self.write_buf.clear();
        }
        Ok(())
    }

    pub async fn write_bytes_batch(&mut self, chunks: &[Bytes]) -> io::Result<()> {
        for chunk in chunks {
            let slice: &[u8] = chunk;
            let mut offset = 0usize;
            while offset < slice.len() {
                let end = (offset + 64 * 1024).min(slice.len());
                self.tls.writer().write_all(&slice[offset..end])?;
                offset = end;

                while self.tls.wants_write() {
                    self.tls.write_tls(&mut self.write_buf)?;
                    if self.write_buf.len() >= BATCH_CIPHERTEXT_FLUSH {
                        self.tcp.write_all(&self.write_buf).await?;
                        self.write_buf.clear();
                    }
                }
            }
        }
        while self.tls.wants_write() {
            self.tls.write_tls(&mut self.write_buf)?;
        }
        if !self.write_buf.is_empty() {
            self.tcp.write_all(&self.write_buf).await?;
            self.write_buf.clear();
        }
        Ok(())
    }

    pub async fn finish(&mut self) -> io::Result<()> {
        self.tls.send_close_notify();
        while self.tls.wants_write() {
            self.tls.write_tls(&mut self.write_buf)?;
        }
        if !self.write_buf.is_empty() {
            self.tcp.write_all(&self.write_buf).await?;
            self.write_buf.clear();
        }
        self.tcp.shutdown().await
    }

    async fn ensure_bytes(&mut self, needed: usize) -> core_protocol::Result<bool> {
        if self.plain_buf.len() >= needed {
            return Ok(true);
        }
        self.plain_buf.reserve(needed - self.plain_buf.len());

        while self.plain_buf.len() < needed {
            loop {
                match self.tls.reader().read(&mut self.decrypt_scratch) {
                    Ok(0) => break,
                    Ok(n) => self.plain_buf.extend_from_slice(&self.decrypt_scratch[..n]),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(ProtocolError::Io(e)),
                }
            }
            if self.plain_buf.len() >= needed {
                return Ok(true);
            }
            if self.eof {
                if self.plain_buf.is_empty() {
                    return Ok(false);
                }
                return Err(ProtocolError::UnexpectedEof);
            }

            while self.tls.wants_write() {
                self.tls
                    .write_tls(&mut self.write_buf)
                    .map_err(ProtocolError::Io)?;
            }
            if !self.write_buf.is_empty() {
                self.tcp
                    .write_all(&self.write_buf)
                    .await
                    .map_err(ProtocolError::Io)?;
                self.write_buf.clear();
            }

            let n = self
                .tcp
                .read(&mut self.net_read_buf)
                .await
                .map_err(ProtocolError::Io)?;
            if n == 0 {
                self.eof = true;
                continue;
            }

            let mut cursor = io::Cursor::new(&self.net_read_buf[..n]);
            while (cursor.position() as usize) < n {
                self.tls
                    .read_tls(&mut cursor)
                    .map_err(ProtocolError::Io)?;
                let state = self.tls.process_new_packets().map_err(|e| {
                    ProtocolError::Io(io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
                })?;
                if state.plaintext_bytes_to_read() > 0 {
                    loop {
                        match self.tls.reader().read(&mut self.decrypt_scratch) {
                            Ok(0) => break,
                            Ok(dec_n) => {
                                self.plain_buf
                                    .extend_from_slice(&self.decrypt_scratch[..dec_n]);
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(e) => return Err(ProtocolError::Io(e)),
                        }
                    }
                }
                if state.peer_has_closed() {
                    self.eof = true;
                }
            }
        }
        Ok(true)
    }

    pub async fn read_next_frame(
        &mut self,
        max_size: usize,
    ) -> core_protocol::Result<Option<ZeroCopyDataFrame>> {
        if !self.ensure_bytes(1).await? {
            return Ok(None);
        }
        let tag = self.plain_buf.get_u8();

        match tag {
            DATA_TAG_CHUNK_RESPONSE => {
                if !self.ensure_bytes(38).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let chunk_index = self.plain_buf.get_u32();
                let mut hash = [0u8; 32];
                self.plain_buf.copy_to_slice(&mut hash);
                let proof_len = self.plain_buf.get_u16() as usize;
                if proof_len > 64 {
                    return Err(ProtocolError::FrameTooLarge {
                        size: proof_len * 32,
                        max: 64 * 32,
                    });
                }

                let proof_bytes_len = proof_len * 32;
                if !self.ensure_bytes(proof_bytes_len + 4).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let mut proof = Vec::with_capacity(proof_len);
                for _ in 0..proof_len {
                    let mut node = [0u8; 32];
                    self.plain_buf.copy_to_slice(&mut node);
                    proof.push(node);
                }
                let data_len = self.plain_buf.get_u32() as usize;
                let total_len = 1 + 38 + proof_bytes_len + 4 + data_len;
                if total_len > max_size {
                    return Err(ProtocolError::FrameTooLarge {
                        size: total_len,
                        max: max_size,
                    });
                }

                if data_len > 0 && !self.ensure_bytes(data_len).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let data = self.plain_buf.split_to(data_len).freeze();
                Ok(Some(ZeroCopyDataFrame::ChunkResponse {
                    chunk_index,
                    hash,
                    proof,
                    data,
                }))
            }
            DATA_TAG_POSTCARD_FALLBACK => {
                if !self.ensure_bytes(4).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let frame_len = self.plain_buf.get_u32() as usize;
                if frame_len > max_size {
                    return Err(ProtocolError::FrameTooLarge {
                        size: frame_len,
                        max: max_size,
                    });
                }
                if frame_len > 0 && !self.ensure_bytes(frame_len).await? {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let payload = self.plain_buf.split_to(frame_len);
                let decoded: DataMessage = postcard::from_bytes(&payload)?;
                Ok(Some(ZeroCopyDataFrame::Control(decoded)))
            }
            other => Err(ProtocolError::InvalidSignalingPacket(format!(
                "unknown LAN data frame tag 0x{other:02x}"
            ))),
        }
    }
}

fn bind_tuned_tcp_listener(
    bind_addr: SocketAddr,
    buffer_ladder: &[usize],
) -> io::Result<std::net::TcpListener> {
    let try_one = |target: SocketAddr, dual_stack: bool| -> io::Result<std::net::TcpListener> {
        let domain = Domain::for_address(target);
        let sock = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        let _ = sock.set_reuse_address(true);
        sock.set_nonblocking(true)?;
        if target.is_ipv6() && dual_stack {
            let _ = sock.set_only_v6(false);
        }
        for &size in buffer_ladder {
            if sock.set_recv_buffer_size(size).is_ok() {
                break;
            }
        }
        for &size in buffer_ladder {
            if sock.set_send_buffer_size(size).is_ok() {
                break;
            }
        }
        sock.bind(&target.into())?;
        sock.listen(128)?;
        Ok(sock.into())
    };

    if bind_addr.is_ipv6() && bind_addr.ip().is_unspecified() {
        if let Ok(listener) = try_one(bind_addr, true) {
            return Ok(listener);
        }
        let v4_fallback = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), bind_addr.port());
        return try_one(v4_fallback, false);
    }

    try_one(bind_addr, false)
}

async fn connect_tuned_tcp_stream(
    target_addr: SocketAddr,
    buffer_ladder: &[usize],
) -> io::Result<TcpStream> {
    let domain = Domain::for_address(target_addr);
    let sock = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    sock.set_nonblocking(true)?;
    let _ = sock.set_nodelay(true);
    for &size in buffer_ladder {
        if sock.set_recv_buffer_size(size).is_ok() {
            break;
        }
    }
    for &size in buffer_ladder {
        if sock.set_send_buffer_size(size).is_ok() {
            break;
        }
    }
    let tokio_sock = tokio::net::TcpSocket::from_std_stream(sock.into());
    tokio_sock.connect(target_addr).await
}

fn tune_accepted_tcp_buffers(tcp: &TcpStream, buffer_ladder: &[usize]) {
    let sock_ref = socket2::SockRef::from(tcp);
    for &size in buffer_ladder {
        if sock_ref.set_recv_buffer_size(size).is_ok() {
            break;
        }
    }
    for &size in buffer_ladder {
        if sock_ref.set_send_buffer_size(size).is_ok() {
            break;
        }
    }
}
