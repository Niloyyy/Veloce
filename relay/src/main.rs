use std::net::SocketAddr;

use veloce_relay::{RelayConfig, RelayServer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bind_str = String::from("0.0.0.0:7878");
    let mut cfg = RelayConfig::default();

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
                eprintln!("  --bind, -b <IP:PORT>       Listen address (default: 0.0.0.0:7878)");
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

    tokio::signal::ctrl_c().await?;
    eprintln!("Shutting down Veloce relay...");
    server.shutdown();
    Ok(())
}
