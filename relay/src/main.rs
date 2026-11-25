use std::net::SocketAddr;

use veloce_relay::{RelayConfig, RelayServer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bind_str =
        std::env::var("VELOCE_RELAY_BIND").unwrap_or_else(|_| String::from("0.0.0.0:7878"));
    let mut cfg = RelayConfig::default();

    if let Ok(v) = std::env::var("VELOCE_RELAY_MAX_PEERS") {
        if let Ok(parsed) = v.parse() {
            cfg.max_total_peers = parsed;
        }
    }
    if let Ok(v) = std::env::var("VELOCE_RELAY_MAX_PEERS_PER_IP") {
        if let Ok(parsed) = v.parse() {
            cfg.max_peers_per_ip = parsed;
        }
    }
    if let Ok(v) = std::env::var("VELOCE_RELAY_MAX_MBPS_PER_IP") {
        if let Ok(mbps) = v.parse::<f64>() {
            let bytes_per_sec = (mbps * 1_000_000.0) / 8.0;
            cfg.max_bytes_per_sec_per_ip = bytes_per_sec;
            cfg.byte_burst_per_ip = bytes_per_sec * 2.0;
        }
    }

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" | "-b" => {
                if let Some(val) = args.next() {
                    bind_str = val;
                }
            }
            "--max-peers" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.max_total_peers = v;
                }
            }
            "--max-peers-per-ip" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.max_peers_per_ip = v;
                }
            }
            "--max-mbps-per-ip" => {
                if let Some(mbps) = args.next().and_then(|s| s.parse::<f64>().ok()) {
                    let bytes_per_sec = (mbps * 1_000_000.0) / 8.0;
                    cfg.max_bytes_per_sec_per_ip = bytes_per_sec;
                    cfg.byte_burst_per_ip = bytes_per_sec * 2.0;
                }
            }
            "--help" | "-h" => {
                eprintln!("veloce-relay — hardened stateless STUN, NAT hole-punching & encrypted UDP relay");
                eprintln!("Usage: veloce-relay [OPTIONS]");
                eprintln!("  --bind, -b <IP:PORT>       Listen address (default: $VELOCE_RELAY_BIND or 0.0.0.0:7878)");
                eprintln!("  --max-peers <N>            Global peer registration cap (default: 10000)");
                eprintln!("  --max-peers-per-ip <N>     Per-IP peer registration cap (default: 16)");
                eprintln!("  --max-mbps-per-ip <MBPS>   Per-IP bandwidth rate limit in Mbps (default: 64)");
                return Ok(());
            }
            _ => {}
        }
    }

    let bind_addr: SocketAddr = bind_str.parse()?;
    let server = RelayServer::bind_with_config(bind_addr, cfg.clone()).await?;
    eprintln!(
        "Veloce hardened stateless relay listening on udp://{} (max_peers={}, max_peers_per_ip={}, max_datagram={}B)",
        server.local_addr(),
        cfg.max_total_peers,
        cfg.max_peers_per_ip,
        cfg.max_datagram_bytes
    );

    wait_for_shutdown_signal().await?;
    eprintln!("Shutting down Veloce relay...");
    server.shutdown();
    Ok(())
}

async fn wait_for_shutdown_signal() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())?;
        tokio::select! {
            res = tokio::signal::ctrl_c() => { res?; }
            _ = sigterm.recv() => {}
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok(())
    }
}
