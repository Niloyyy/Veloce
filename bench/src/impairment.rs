use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio_util::sync::CancellationToken;

/// Standard Ethernet TCP Maximum Segment Size (bytes).
const TCP_MSS_BYTES: usize = 1460;

/// Parameters for simulated WAN network impairment (default: 100ms RTT, 1% packet loss).
#[derive(Debug, Clone, Copy)]
pub struct NetworkProfile {
    /// Round-trip latency in milliseconds (split evenly 50/50 across forward & reverse paths).
    pub rtt_ms: u64,
    /// Per-packet/segment drop probability on the data path (0.0 .. 1.0).
    pub loss_rate: f64,
    /// Deterministic PRNG seed for reproducible benchmarks.
    pub seed: u64,
}

impl Default for NetworkProfile {
    fn default() -> Self {
        Self {
            rtt_ms: 100,
            loss_rate: 0.01,
            seed: 0x5645_4C4F_4345_0001,
        }
    }
}

#[derive(Debug, Default)]
pub struct ProxyStats {
    pub packets_forwarded: AtomicU64,
    pub packets_dropped: AtomicU64,
    pub bytes_forwarded: AtomicU64,
}

/// Bidirectional userspace UDP impairment proxy injecting deterministic one-way delay
/// and independent packet drops for genuine QUIC BBR loss-recovery benchmarking without root.
pub struct ImpairedUdpProxy {
    client_facing_addr: SocketAddr,
    stats: Arc<ProxyStats>,
    cancel: CancellationToken,
}

impl ImpairedUdpProxy {
    pub async fn start(
        upstream_server: SocketAddr,
        profile: NetworkProfile,
    ) -> std::io::Result<Self> {
        let front = Arc::new(bind_large_udp("127.0.0.1:0".parse().unwrap())?);
        let back = Arc::new(bind_large_udp("127.0.0.1:0".parse().unwrap())?);
        let client_facing_addr = front.local_addr()?;

        let stats = Arc::new(ProxyStats::default());
        let cancel = CancellationToken::new();
        let client_addr: Arc<tokio::sync::RwLock<Option<SocketAddr>>> =
            Arc::new(tokio::sync::RwLock::new(None));

        let one_way_delay = Duration::from_millis(profile.rtt_ms / 2);
        let server_to_client_loss = profile.loss_rate;
        let client_to_server_loss = profile.loss_rate * 0.25;

        // Client -> Server direction.
        {
            let front_rx = Arc::clone(&front);
            let back_tx = Arc::clone(&back);
            let c_addr = Arc::clone(&client_addr);
            let st = Arc::clone(&stats);
            let tok = cancel.clone();
            tokio::spawn(async move {
                let mut rng = StdRng::seed_from_u64(profile.seed);
                let mut buf = vec![0u8; 65_535];
                loop {
                    tokio::select! {
                        _ = tok.cancelled() => break,
                        recv = front_rx.recv_from(&mut buf) => {
                            let (n, src) = match recv {
                                Ok(v) => v,
                                Err(_) => break,
                            };
                            {
                                let mut guard = c_addr.write().await;
                                *guard = Some(src);
                            }
                            if client_to_server_loss > 0.0 && rng.gen::<f64>() < client_to_server_loss {
                                st.packets_dropped.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                            let payload = buf[..n].to_vec();
                            let tx = Arc::clone(&back_tx);
                            let st_inner = Arc::clone(&st);
                            let deliver_at = Instant::now() + one_way_delay;
                            tokio::spawn(async move {
                                let now = Instant::now();
                                if deliver_at > now {
                                    tokio::time::sleep(deliver_at - now).await;
                                }
                                if tx.send_to(&payload, upstream_server).await.is_ok() {
                                    st_inner.packets_forwarded.fetch_add(1, Ordering::Relaxed);
                                    st_inner.bytes_forwarded.fetch_add(payload.len() as u64, Ordering::Relaxed);
                                }
                            });
                        }
                    }
                }
            });
        }

        // Server -> Client direction (primary bulk data path).
        {
            let back_rx = Arc::clone(&back);
            let front_tx = Arc::clone(&front);
            let c_addr = Arc::clone(&client_addr);
            let st = Arc::clone(&stats);
            let tok = cancel.clone();
            tokio::spawn(async move {
                let mut rng = StdRng::seed_from_u64(profile.seed ^ 0xA5A5_5A5A);
                let mut buf = vec![0u8; 65_535];
                loop {
                    tokio::select! {
                        _ = tok.cancelled() => break,
                        recv = back_rx.recv_from(&mut buf) => {
                            let (n, _src) = match recv {
                                Ok(v) => v,
                                Err(_) => break,
                            };
                            let Some(dst) = *c_addr.read().await else {
                                continue;
                            };
                            if server_to_client_loss > 0.0 && rng.gen::<f64>() < server_to_client_loss {
                                st.packets_dropped.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                            let payload = buf[..n].to_vec();
                            let tx = Arc::clone(&front_tx);
                            let st_inner = Arc::clone(&st);
                            let deliver_at = Instant::now() + one_way_delay;
                            tokio::spawn(async move {
                                let now = Instant::now();
                                if deliver_at > now {
                                    tokio::time::sleep(deliver_at - now).await;
                                }
                                if tx.send_to(&payload, dst).await.is_ok() {
                                    st_inner.packets_forwarded.fetch_add(1, Ordering::Relaxed);
                                    st_inner.bytes_forwarded.fetch_add(payload.len() as u64, Ordering::Relaxed);
                                }
                            });
                        }
                    }
                }
            });
        }

        Ok(Self {
            client_facing_addr,
            stats,
            cancel,
        })
    }

    pub fn proxy_addr(&self) -> SocketAddr {
        self.client_facing_addr
    }

    pub fn stats(&self) -> &Arc<ProxyStats> {
        &self.stats
    }

    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

impl Drop for ImpairedUdpProxy {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Empirical userspace TCP impairment proxy routing unmodified real `TcpStream` sender and
/// receiver sockets through a real TCP relay that injects propagation RTT, MSS-level loss,
/// single-stream Head-of-Line retransmission stalls, and Cubic/AIMD window backpressure.
///
/// Using bounded socket receive buffers on the proxy leg ensures the sender's kernel `TcpStream`
/// genuinely blocks on flow control rather than absorbing megabytes into loopback kernel memory.
pub struct ImpairedTcpProxy {
    listen_addr: SocketAddr,
    stats: Arc<ProxyStats>,
    cancel: CancellationToken,
}

impl ImpairedTcpProxy {
    pub async fn start(
        upstream_server: SocketAddr,
        profile: NetworkProfile,
    ) -> std::io::Result<Self> {
        let listener = bind_bounded_tcp_listener("127.0.0.1:0".parse().unwrap(), 64 * 1024)?;
        let listen_addr = listener.local_addr()?;
        let stats = Arc::new(ProxyStats::default());
        let cancel = CancellationToken::new();

        let st = Arc::clone(&stats);
        let tok = cancel.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tok.cancelled() => break,
                    accepted = listener.accept() => {
                        let (mut client_sock, _) = match accepted {
                            Ok(v) => v,
                            Err(_) => break,
                        };
                        let _ = client_sock.set_nodelay(true);
                        let st_conn = Arc::clone(&st);
                        let tok_conn = tok.clone();
                        tokio::spawn(async move {
                            // 3-way TCP handshake traverses 1 RTT.
                            if profile.rtt_ms > 0 {
                                tokio::time::sleep(Duration::from_millis(profile.rtt_ms)).await;
                            }
                            let Ok(mut upstream_sock) = TcpStream::connect(upstream_server).await else {
                                return;
                            };
                            let _ = upstream_sock.set_nodelay(true);

                            let mut rng = StdRng::seed_from_u64(profile.seed);
                            let rtt = Duration::from_millis(profile.rtt_ms);
                            // RFC 6928 initial cwnd = 10 MSS, Cubic multiplicative decrease beta = 0.7.
                            let mut cwnd_segs: f64 = 10.0;
                            let mut ssthresh: f64 = 64.0;
                            let mut seg_buf = vec![0u8; TCP_MSS_BYTES];

                            'outer: loop {
                                let window_segs = (cwnd_segs.floor() as usize).max(2);
                                let mut forwarded_in_window = 0usize;
                                let mut loss_in_window = false;
                                let window_start = Instant::now();

                                for _ in 0..window_segs {
                                    let n = tokio::select! {
                                        _ = tok_conn.cancelled() => break 'outer,
                                        r = client_sock.read(&mut seg_buf) => match r {
                                            Ok(0) => break 'outer,
                                            Ok(n) => n,
                                            Err(_) => break 'outer,
                                        }
                                    };

                                    if profile.loss_rate > 0.0 && rng.gen::<f64>() < profile.loss_rate {
                                        // Segment lost: single-stream TCP triggers 1-RTT HoL fast retransmit
                                        // before downstream bytes can be delivered in-order to the app.
                                        st_conn.packets_dropped.fetch_add(1, Ordering::Relaxed);
                                        loss_in_window = true;
                                        if !rtt.is_zero() {
                                            tokio::time::sleep(rtt).await;
                                        }
                                    }

                                    if upstream_sock.write_all(&seg_buf[..n]).await.is_err() {
                                        break 'outer;
                                    }
                                    st_conn.packets_forwarded.fetch_add(1, Ordering::Relaxed);
                                    st_conn.bytes_forwarded.fetch_add(n as u64, Ordering::Relaxed);
                                    forwarded_in_window += 1;

                                    if loss_in_window {
                                        break;
                                    }
                                }

                                // Pace ACK clock by 1 RTT so the real client TcpStream experiences
                                // genuine BDP-limited flow control.
                                if !rtt.is_zero() {
                                    let spent = window_start.elapsed();
                                    if rtt > spent {
                                        tokio::time::sleep(rtt - spent).await;
                                    }
                                }

                                if loss_in_window {
                                    ssthresh = (cwnd_segs * 0.7).max(2.0);
                                    cwnd_segs = ssthresh;
                                } else if cwnd_segs < ssthresh {
                                    cwnd_segs = (cwnd_segs + forwarded_in_window as f64).min(ssthresh);
                                } else {
                                    cwnd_segs += 1.0;
                                }
                            }
                            let _ = upstream_sock.shutdown().await;
                        });
                    }
                }
            }
        });

        Ok(Self {
            listen_addr,
            stats,
            cancel,
        })
    }

    pub fn proxy_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn stats(&self) -> &Arc<ProxyStats> {
        &self.stats
    }

    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

impl Drop for ImpairedTcpProxy {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn bind_large_udp(addr: SocketAddr) -> std::io::Result<UdpSocket> {
    let sock = Socket::new(Domain::for_address(addr), Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_nonblocking(true)?;
    for size in [8 * 1024 * 1024, 4 * 1024 * 1024, 2 * 1024 * 1024] {
        if sock.set_recv_buffer_size(size).is_ok() {
            break;
        }
    }
    for size in [8 * 1024 * 1024, 4 * 1024 * 1024, 2 * 1024 * 1024] {
        if sock.set_send_buffer_size(size).is_ok() {
            break;
        }
    }
    sock.bind(&addr.into())?;
    UdpSocket::from_std(sock.into())
}

fn bind_bounded_tcp_listener(addr: SocketAddr, recv_buf: usize) -> std::io::Result<TcpListener> {
    let sock = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    sock.set_nonblocking(true)?;
    let _ = sock.set_reuse_address(true);
    let _ = sock.set_recv_buffer_size(recv_buf);
    sock.bind(&addr.into())?;
    sock.listen(16)?;
    TcpListener::from_std(sock.into())
}
