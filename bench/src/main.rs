mod impairment;

use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use core_protocol::ConnectionType;
use core_storage::ChunkSource;
use core_transport::{EndpointOptions, VeloceEndpoint};
use impairment::{ImpairedTcpProxy, ImpairedUdpProxy, NetworkProfile};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone)]
struct BenchConfig {
    /// Clean LAN steady-state payload size in MiB (default: 64 MiB).
    lan_size_mb: usize,
    /// Impaired WAN payload size in MiB (default: 8 MiB, or overridden with --wan-size-mb / --size-mb).
    wan_size_mb: usize,
    /// Optional wall-clock steady-state cap in seconds for slow WAN TCP runs (0 = run to completion).
    wan_tcp_max_secs: u64,
    rtt_ms: u64,
    loss_pct: f64,
    quic_streams: usize,
    chunk_kb: u32,
    direct_netem: bool,
    bind_ip: Ipv4Addr,
    peer_ip: Ipv4Addr,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            lan_size_mb: 64,
            wan_size_mb: 8,
            wan_tcp_max_secs: 15,
            rtt_ms: 100,
            loss_pct: 1.0,
            quic_streams: 8,
            chunk_kb: 1024,
            direct_netem: false,
            bind_ip: Ipv4Addr::LOCALHOST,
            peer_ip: Ipv4Addr::LOCALHOST,
        }
    }
}

#[derive(Debug)]
struct BenchResult {
    label: String,
    bytes: u64,
    total_elapsed: Duration,
    steady_elapsed: Duration,
    retransmits_or_drops: u64,
    note: String,
}

impl BenchResult {
    fn total_mb_per_sec(&self) -> f64 {
        let mb = (self.bytes as f64) / (1024.0 * 1024.0);
        mb / self.total_elapsed.as_secs_f64().max(0.0001)
    }

    fn steady_mb_per_sec(&self) -> f64 {
        let mb = (self.bytes as f64) / (1024.0 * 1024.0);
        mb / self.steady_elapsed.as_secs_f64().max(0.0001)
    }
}

fn parse_args() -> BenchConfig {
    let mut cfg = BenchConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--size-mb" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.lan_size_mb = v;
                    cfg.wan_size_mb = v;
                }
            }
            "--lan-size-mb" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.lan_size_mb = v;
                }
            }
            "--wan-size-mb" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.wan_size_mb = v;
                }
            }
            "--wan-tcp-max-secs" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.wan_tcp_max_secs = v;
                }
            }
            "--rtt-ms" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.rtt_ms = v;
                }
            }
            "--loss-pct" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.loss_pct = v;
                }
            }
            "--streams" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.quic_streams = v;
                }
            }
            "--chunk-kb" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.chunk_kb = v;
                }
            }
            "--direct-netem" => {
                cfg.direct_netem = true;
            }
            "--bind-ip" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.bind_ip = v;
                }
            }
            "--peer-ip" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.peer_ip = v;
                }
            }
            "--help" | "-h" => {
                println!("veloce-bench — Single-Stream TCP vs Multi-Stream QUIC (BBR) Benchmark");
                println!("Flags:");
                println!("  --size-mb <N>          Set both LAN and WAN payload size in MiB");
                println!("  --lan-size-mb <N>      Clean LAN payload size in MiB (default: 64)");
                println!("  --wan-size-mb <N>      Impaired WAN payload size in MiB (default: 8)");
                println!("  --wan-tcp-max-secs <S> Time cap for slow WAN TCP (default: 15, 0=none)");
                println!("  --rtt-ms <MS>          Simulated RTT in ms (default: 100)");
                println!("  --loss-pct <PCT>       Simulated packet loss % (default: 1.0)");
                println!("  --streams <N>          Concurrent QUIC streams (default: 8)");
                println!("  --chunk-kb <KB>        Chunk size in KiB (default: 1024)");
                println!("  --direct-netem         Disable userspace proxies (use kernel tc netem)");
                std::process::exit(0);
            }
            _ => {}
        }
    }
    cfg
}

/// Runs an unmodified real single-stream TCP transfer, either directly over loopback/netem
/// or routed empirically through `ImpairedTcpProxy` (non-root WAN mode).
async fn bench_single_stream_tcp(
    payload: &[u8],
    bind_ip: Ipv4Addr,
    peer_ip: Ipv4Addr,
    profile: Option<NetworkProfile>,
    max_duration: Option<Duration>,
) -> std::io::Result<BenchResult> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(bind_ip), 0)).await?;
    let server_addr = listener.local_addr()?;

    let proxy = if let Some(p) = profile {
        Some(ImpairedTcpProxy::start(server_addr, p).await?)
    } else {
        None
    };

    let connect_addr = proxy
        .as_ref()
        .map(ImpairedTcpProxy::proxy_addr)
        .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(peer_ip), server_addr.port()));

    let expected_hash = *blake3::hash(payload).as_bytes();
    let total_len = payload.len() as u64;

    let server_task = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = sock.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            total += n as u64;
        }
        Ok::<(u64, [u8; 32]), std::io::Error>((total, *hasher.finalize().as_bytes()))
    });

    let start = Instant::now();
    let mut client = TcpStream::connect(connect_addr).await?;
    client.set_nodelay(true)?;
    let data_start = Instant::now();

    let mut time_capped = false;
    if let Some(limit) = max_duration {
        match tokio::time::timeout(limit, client.write_all(payload)).await {
            Ok(res) => res?,
            Err(_) => time_capped = true,
        }
    } else {
        client.write_all(payload).await?;
    }

    let _ = client.shutdown().await;
    drop(client);

    if time_capped {
        if let Some(ref p) = proxy {
            p.shutdown();
        }
    }

    let (rx_bytes, rx_hash) = server_task.await.unwrap()?;
    let total_elapsed = start.elapsed();
    let steady_elapsed = data_start.elapsed();

    let drops = proxy
        .as_ref()
        .map(|p| p.stats().packets_dropped.load(Ordering::Relaxed))
        .unwrap_or(0);
    if let Some(p) = proxy {
        p.shutdown();
    }

    if !time_capped {
        assert_eq!(rx_bytes, total_len);
        assert_eq!(rx_hash, expected_hash);
    }

    let note = if time_capped {
        format!(
            "steady-state sample ({:.2} MiB in {:.1}s)",
            rx_bytes as f64 / (1024.0 * 1024.0),
            steady_elapsed.as_secs_f64()
        )
    } else {
        "full payload verified".into()
    };

    Ok(BenchResult {
        label: if profile.is_some() {
            "Single-Stream TCP (Impaired Proxy)".into()
        } else {
            "Single-Stream TCP (Direct Kernel)".into()
        },
        bytes: rx_bytes,
        total_elapsed,
        steady_elapsed,
        retransmits_or_drops: drops,
        note,
    })
}

/// Runs Veloce's real multi-stream QUIC (BBR) + BLAKE3 Merkle chunking transfer,
/// reporting both total elapsed (handshake + data + fsync) and pure steady-state streaming speed.
async fn bench_multistream_quic_bbr(
    payload: &[u8],
    chunk_size: u32,
    streams: usize,
    bind_ip: Ipv4Addr,
    peer_ip: Ipv4Addr,
    profile: Option<NetworkProfile>,
) -> Result<BenchResult, Box<dyn std::error::Error>> {
    let src_dir = tempdir()?;
    let dst_dir = tempdir()?;
    let src_file = src_dir.path().join("bench_payload.bin");
    fs::write(&src_file, payload)?;

    let hash_start = Instant::now();
    let source = ChunkSource::from_paths_async(vec![src_file], Some(chunk_size)).await?;
    let hash_ms = hash_start.elapsed().as_millis();

    let sender_opts = EndpointOptions {
        bind_addr: SocketAddr::new(IpAddr::V4(bind_ip), 0),
        device_name: "BenchSender".into(),
        enable_mdns: false,
        parallel_streams: streams,
        relay_addrs: Vec::new(),
        force_relay: false,
    };
    let receiver_opts = EndpointOptions {
        bind_addr: SocketAddr::new(IpAddr::V4(peer_ip), 0),
        device_name: "BenchReceiver".into(),
        enable_mdns: false,
        parallel_streams: streams,
        relay_addrs: Vec::new(),
        force_relay: false,
    };

    let (sender, _s_ch) = VeloceEndpoint::bind(sender_opts).await?;
    let (receiver, _r_ch) = VeloceEndpoint::bind(receiver_opts).await?;

    let hosted = sender.host_session(source.clone(), vec![sender.listen_addr()], vec![])?;

    let (dial_addr, proxy) = if let Some(p) = profile {
        let proxy = ImpairedUdpProxy::start(sender.listen_addr(), p).await?;
        (proxy.proxy_addr(), Some(proxy))
    } else {
        (sender.listen_addr(), None)
    };

    let start = Instant::now();
    let summary = receiver
        .pull_session_direct(
            &hosted.ticket,
            dial_addr,
            dst_dir.path().to_path_buf(),
            streams,
            ConnectionType::DirectP2p,
        )
        .await?;
    let total_elapsed = start.elapsed();

    let drops = proxy
        .as_ref()
        .map(|p| p.stats().packets_dropped.load(Ordering::Relaxed))
        .unwrap_or(0);

    let received = fs::read(dst_dir.path().join("bench_payload.bin"))?;
    assert_eq!(received.len(), payload.len());
    assert_eq!(blake3::hash(&received), blake3::hash(payload));

    if let Some(p) = proxy {
        p.shutdown();
    }
    sender.shutdown();
    receiver.shutdown();

    let steady_elapsed = Duration::from_millis(summary.data_elapsed_ms);
    Ok(BenchResult {
        label: format!("Multi-Stream QUIC BBR ({streams} streams)"),
        bytes: summary.total_bytes,
        total_elapsed,
        steady_elapsed,
        retransmits_or_drops: drops,
        note: format!("index/hash={}ms, full Merkle verified", hash_ms),
    })
}

fn make_payload(size_mb: usize) -> Vec<u8> {
    let len = size_mb * 1024 * 1024;
    (0..len)
        .map(|i| ((i.wrapping_mul(31) ^ (i >> 7)) & 0xFF) as u8)
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = parse_args();

    println!("===========================================================================================");
    println!("  VELOCE TRANSPORT BENCHMARK: Single-Stream TCP vs Multi-Stream QUIC (BBR)");
    println!("===========================================================================================");
    println!(
        "  LAN Payload  : {} MiB steady-state | WAN Payload: {} MiB",
        cfg.lan_size_mb, cfg.wan_size_mb
    );
    println!(
        "  Impairment   : {} ms RTT, {:.2}% packet loss ({})",
        cfg.rtt_ms,
        cfg.loss_pct,
        if cfg.direct_netem {
            "kernel tc netem"
        } else {
            "empirical userspace TCP & UDP proxies"
        }
    );
    println!(
        "  QUIC Config  : BBR + 1380B MTU + {} pipelined streams + {} KiB chunks + 8 MiB UDP bufs",
        cfg.quic_streams, cfg.chunk_kb
    );
    println!("-------------------------------------------------------------------------------------------");

    // 1. Clean LAN steady-state benchmark (default 64 MiB).
    if !cfg.direct_netem {
        let lan_payload = make_payload(cfg.lan_size_mb);
        let lan_tcp =
            bench_single_stream_tcp(&lan_payload, cfg.bind_ip, cfg.peer_ip, None, None).await?;
        let lan_quic = bench_multistream_quic_bbr(
            &lan_payload,
            cfg.chunk_kb * 1024,
            cfg.quic_streams,
            cfg.bind_ip,
            cfg.peer_ip,
            None,
        )
        .await?;
        println!(
            "\n[1. Clean LAN Loopback — {} MiB, 0ms RTT, 0.00% Loss]",
            cfg.lan_size_mb
        );
        print_table(&[lan_tcp, lan_quic]);
    }

    // 2. Impaired WAN Long-Fat-Network benchmark (default 100ms RTT, 1% packet loss).
    let wan_payload = make_payload(cfg.wan_size_mb);
    let profile = if cfg.direct_netem {
        None
    } else {
        Some(NetworkProfile {
            rtt_ms: cfg.rtt_ms,
            loss_rate: cfg.loss_pct / 100.0,
            ..Default::default()
        })
    };
    let tcp_cap = if cfg.wan_tcp_max_secs > 0 && !cfg.direct_netem {
        Some(Duration::from_secs(cfg.wan_tcp_max_secs))
    } else {
        None
    };

    let wan_tcp =
        bench_single_stream_tcp(&wan_payload, cfg.bind_ip, cfg.peer_ip, profile, tcp_cap).await?;
    let wan_quic = bench_multistream_quic_bbr(
        &wan_payload,
        cfg.chunk_kb * 1024,
        cfg.quic_streams,
        cfg.bind_ip,
        cfg.peer_ip,
        profile,
    )
    .await?;

    println!(
        "\n[2. Impaired WAN (LFN) — {} MiB, {}ms RTT, {:.2}% Packet Loss]",
        cfg.wan_size_mb, cfg.rtt_ms, cfg.loss_pct
    );
    let speedup = wan_quic.steady_mb_per_sec() / wan_tcp.steady_mb_per_sec().max(0.0001);
    print_table(&[wan_tcp, wan_quic]);
    println!(
        "\n=> Multi-Stream QUIC (BBR) Steady-State Speedup over Single-Stream TCP: {:.2}x",
        speedup
    );
    println!("===========================================================================================");

    Ok(())
}

fn print_table(results: &[BenchResult]) {
    println!(
        "{:<36} | {:>9} | {:>12} | {:>12} | {:>6} | Notes",
        "Transport", "Total", "Total Speed", "Steady-State", "Drops"
    );
    println!(
        "{:-<36}-+-{:-<9}-+-{:-<12}-+-{:-<12}-+-{:-<6}-+-{:-<24}",
        "", "", "", "", "", ""
    );
    for r in results {
        println!(
            "{:<36} | {:>7.2} s | {:>7.2} MB/s | {:>7.2} MB/s | {:>6} | {}",
            r.label,
            r.total_elapsed.as_secs_f64(),
            r.total_mb_per_sec(),
            r.steady_mb_per_sec(),
            r.retransmits_or_drops,
            r.note
        );
    }
}
