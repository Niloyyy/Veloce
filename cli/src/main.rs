use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use core_ffi::{EngineConfigDto, EngineEvent, VeloceEngine};
use qrcode::render::unicode;
use qrcode::QrCode;
use tokio::sync::broadcast;

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Send {
        paths: Vec<String>,
        device_name: String,
        bind_addr: Option<String>,
        relays: Vec<String>,
        streams: u32,
        no_qr: bool,
        once: bool,
    },
    Receive {
        ticket: String,
        output_dir: PathBuf,
        device_name: String,
        relays: Vec<String>,
        streams: u32,
        force_relay: bool,
    },
    Peers {
        timeout_secs: u64,
        device_name: String,
    },
    Help,
}

fn default_device_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "Veloce CLI".into())
}

fn parse_cli<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = args.into_iter();
    let Some(sub) = iter.next() else {
        return Ok(Command::Help);
    };

    match sub.as_str() {
        "send" => {
            let mut paths = Vec::new();
            let mut device_name = default_device_name();
            let mut bind_addr = None;
            let mut relays = Vec::new();
            let mut streams = 8u32;
            let mut no_qr = false;
            let mut once = false;

            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "--name" | "-n" => {
                        device_name = iter.next().ok_or("missing value for --name")?;
                    }
                    "--bind" | "-b" => {
                        bind_addr = Some(iter.next().ok_or("missing value for --bind")?);
                    }
                    "--relay" | "-r" => {
                        relays.push(iter.next().ok_or("missing value for --relay")?);
                    }
                    "--streams" | "-s" => {
                        streams = iter
                            .next()
                            .ok_or("missing value for --streams")?
                            .parse()
                            .map_err(|_| "invalid integer for --streams")?;
                    }
                    "--no-qr" => no_qr = true,
                    "--once" => once = true,
                    "--help" | "-h" => return Ok(Command::Help),
                    other if other.starts_with('-') => {
                        return Err(format!("unknown flag for send: {other}"));
                    }
                    path => paths.push(path.to_string()),
                }
            }

            if paths.is_empty() {
                return Err("veloce send requires at least one file or directory path".into());
            }

            Ok(Command::Send {
                paths,
                device_name,
                bind_addr,
                relays,
                streams,
                no_qr,
                once,
            })
        }
        "receive" | "recv" => {
            let mut ticket: Option<String> = None;
            let mut output_dir = PathBuf::from(".");
            let mut device_name = default_device_name();
            let mut relays = Vec::new();
            let mut streams = 8u32;
            let mut force_relay = false;

            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "-o" | "--output" => {
                        output_dir = PathBuf::from(iter.next().ok_or("missing value for -o")?);
                    }
                    "--name" | "-n" => {
                        device_name = iter.next().ok_or("missing value for --name")?;
                    }
                    "--relay" | "-r" => {
                        relays.push(iter.next().ok_or("missing value for --relay")?);
                    }
                    "--streams" | "-s" => {
                        streams = iter
                            .next()
                            .ok_or("missing value for --streams")?
                            .parse()
                            .map_err(|_| "invalid integer for --streams")?;
                    }
                    "--force-relay" => force_relay = true,
                    "--help" | "-h" => return Ok(Command::Help),
                    other if other.starts_with('-') => {
                        return Err(format!("unknown flag for receive: {other}"));
                    }
                    val => {
                        if ticket.is_none() {
                            ticket = Some(val.to_string());
                        } else {
                            return Err(format!("unexpected positional argument: {val}"));
                        }
                    }
                }
            }

            let ticket = ticket.ok_or("veloce receive requires a <ticket> argument")?;
            Ok(Command::Receive {
                ticket,
                output_dir,
                device_name,
                relays,
                streams,
                force_relay,
            })
        }
        "peers" => {
            let mut timeout_secs = 3u64;
            let mut device_name = default_device_name();
            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "--timeout" | "-t" => {
                        timeout_secs = iter
                            .next()
                            .ok_or("missing value for --timeout")?
                            .parse()
                            .map_err(|_| "invalid integer for --timeout")?;
                    }
                    "--name" | "-n" => {
                        device_name = iter.next().ok_or("missing value for --name")?;
                    }
                    "--help" | "-h" => return Ok(Command::Help),
                    other => return Err(format!("unknown flag for peers: {other}")),
                }
            }
            Ok(Command::Peers {
                timeout_secs,
                device_name,
            })
        }
        "--help" | "-h" | "help" => Ok(Command::Help),
        other => Err(format!("unknown command '{other}' (try `veloce --help`)")),
    }
}

fn print_help() {
    println!("veloce — High-Performance Local & Global QUIC (BBR) P2P File Transfer CLI");
    println!();
    println!("USAGE:");
    println!("  veloce send <path...> [OPTIONS]");
    println!("  veloce receive <ticket> [-o <dir>] [OPTIONS]");
    println!("  veloce peers [--timeout <secs>]");
    println!();
    println!("SEND OPTIONS:");
    println!("  --name, -n <NAME>        Display name advertised over mDNS & handshake");
    println!("  --relay, -r <HOST:PORT>  Stateless relay hint (repeatable)");
    println!("  --bind, -b <IP:PORT>     UDP bind address (default: 0.0.0.0:0)");
    println!("  --streams, -s <N>        Parallel QUIC streams (default: 8)");
    println!("  --no-qr                  Skip printing terminal QR code");
    println!("  --once                   Exit automatically after the first completed transfer");
    println!();
    println!("RECEIVE OPTIONS:");
    println!("  -o, --output <DIR>       Destination directory (default: current directory)");
    println!("  --relay, -r <HOST:PORT>  Additional relay server hint");
    println!("  --streams, -s <N>        Parallel QUIC streams (default: 8)");
    println!("  --force-relay            Force end-to-end encrypted relay mode");
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == 0 {
        return "0 B".into();
    }
    let exp = (bytes as f64).log(1024.0).floor() as usize;
    let idx = exp.min(UNITS.len() - 1);
    let val = (bytes as f64) / 1024f64.powi(idx as i32);
    format!("{val:.2} {}", UNITS[idx])
}

fn format_speed(bytes_per_sec: f64) -> String {
    let mb = bytes_per_sec / (1024.0 * 1024.0);
    if mb >= 1.0 {
        format!("{mb:.2} MB/s")
    } else {
        format!("{:.0} KB/s", bytes_per_sec / 1024.0)
    }
}

fn format_eta(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "--".into();
    }
    let s = seconds.ceil() as u64;
    if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

fn render_progress_line(
    badge: &str,
    done: u64,
    total: u64,
    chunks_done: u32,
    chunks_total: u32,
    speed: f64,
    eta: f64,
) {
    let pct = if total > 0 {
        ((done as f64) / (total as f64) * 100.0).min(100.0)
    } else {
        100.0
    };
    let bar_width = 24usize;
    let filled = ((pct / 100.0) * (bar_width as f64)).round() as usize;
    let bar: String = format!(
        "{}{}",
        "#".repeat(filled.min(bar_width)),
        "-".repeat(bar_width.saturating_sub(filled))
    );
    print!(
        "\r[{badge}] [{bar}] {pct:5.1}% | {} / {} | {} | ETA {} | {chunks_done}/{chunks_total} chunks   ",
        format_bytes(done),
        format_bytes(total),
        format_speed(speed),
        format_eta(eta)
    );
    let _ = io::stdout().flush();
}

#[tokio::main]
async fn main() {
    let cmd = match parse_cli(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    };

    if let Err(err) = run(cmd).await {
        eprintln!("\nerror: {err}");
        std::process::exit(1);
    }
}

async fn run(cmd: Command) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Command::Help => {
            print_help();
            Ok(())
        }
        Command::Send {
            paths,
            device_name,
            bind_addr,
            relays,
            streams,
            no_qr,
            once,
        } => {
            let engine = VeloceEngine::init(EngineConfigDto {
                device_name,
                bind_addr,
                enable_mdns: Some(true),
                parallel_streams: Some(streams),
                relay_addrs: if relays.is_empty() {
                    None
                } else {
                    Some(relays.clone())
                },
                ..Default::default()
            })
            .await?;

            let info = engine.node_info();
            let mut events = engine.subscribe_events();
            let started = engine.start_session(paths, None, relays).await?;

            println!("Veloce Send Ready");
            println!(
                "  Node      : {} ({}) on udp://{}",
                info.device_name, info.short_id, info.listen_addr
            );
            println!("  LAN mDNS  : advertising _veloce._udp.local.");
            println!(
                "  Payload   : {} file(s), {} ({} chunks of {})",
                started.file_count,
                format_bytes(started.total_size),
                started.total_chunks,
                format_bytes(started.chunk_size as u64)
            );
            println!("\nGlobal Session Ticket:");
            println!("  {}\n", started.ticket_code);

            if !no_qr {
                if let Ok(code) = QrCode::new(started.qr_uri.as_bytes()) {
                    let rendered = code
                        .render::<unicode::Dense1x2>()
                        .dark_color(unicode::Dense1x2::Light)
                        .light_color(unicode::Dense1x2::Dark)
                        .quiet_zone(true)
                        .build();
                    println!("{rendered}\n");
                }
            }

            println!("Waiting for receiver (run `veloce receive <ticket>` or scan QR; Ctrl-C to stop)...");

            let mut active_badge = String::from("LAN");
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        println!("\nShutting down sender...");
                        break;
                    }
                    evt = events.recv() => {
                        match evt {
                            Ok(EngineEvent::ConnectionTypeChanged { connection_type, .. }) => {
                                active_badge = connection_type;
                            }
                            Ok(EngineEvent::TransferProgress {
                                bytes_transferred,
                                total_bytes,
                                chunks_verified,
                                total_chunks,
                                speed_bytes_per_sec,
                                eta_seconds,
                                connection_type,
                                ..
                            }) => {
                                active_badge = connection_type;
                                render_progress_line(
                                    &active_badge,
                                    bytes_transferred,
                                    total_bytes,
                                    chunks_verified,
                                    total_chunks,
                                    speed_bytes_per_sec,
                                    eta_seconds,
                                );
                                if chunks_verified == total_chunks && once {
                                    println!("\nTransfer served completely.");
                                    break;
                                }
                            }
                            Ok(EngineEvent::TransferCompleted { elapsed_ms, average_speed_bytes_per_sec, .. }) => {
                                println!(
                                    "\n[{active_badge}] Completed in {:.2}s (avg {})",
                                    elapsed_ms as f64 / 1000.0,
                                    format_speed(average_speed_bytes_per_sec)
                                );
                                if once {
                                    break;
                                }
                            }
                            Ok(EngineEvent::TransferFailed { error, .. }) => {
                                eprintln!("\nTransfer error: {error}");
                                if once {
                                    break;
                                }
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                            _ => {}
                        }
                    }
                }
            }

            engine.shutdown();
            Ok(())
        }
        Command::Receive {
            ticket,
            output_dir,
            device_name,
            relays,
            streams,
            force_relay,
        } => {
            let preview = VeloceEngine::parse_ticket(&ticket)?;
            println!("Joining Veloce Session");
            println!("  Sender    : {}", preview.sender_short_id);
            println!(
                "  Payload   : {} file(s), {}",
                preview.file_count,
                format_bytes(preview.total_size)
            );
            println!("  Output    : {}", output_dir.display());
            println!("  Root Hash : {}...", &preview.merkle_root_hex[..16]);

            let engine = VeloceEngine::init(EngineConfigDto {
                device_name,
                bind_addr: None,
                enable_mdns: Some(true),
                parallel_streams: Some(streams),
                relay_addrs: if relays.is_empty() {
                    None
                } else {
                    Some(relays)
                },
                force_relay: Some(force_relay),
                ..Default::default()
            })
            .await?;

            let mut events = engine.subscribe_events();
            let joined = engine
                .join_session(ticket, output_dir.display().to_string(), Some(streams))
                .await?;

            let mut badge = String::from("Connecting");
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        engine.pause_transfer(&joined.transfer_id);
                        println!("\nPaused & saved resume state. Re-run the same command to resume.");
                        break;
                    }
                    evt = events.recv() => {
                        match evt {
                            Ok(EngineEvent::ConnectionTypeChanged { connection_type, .. }) => {
                                badge = connection_type;
                            }
                            Ok(EngineEvent::TransferProgress {
                                bytes_transferred,
                                total_bytes,
                                chunks_verified,
                                total_chunks,
                                speed_bytes_per_sec,
                                eta_seconds,
                                connection_type,
                                ..
                            }) => {
                                badge = connection_type;
                                render_progress_line(
                                    &badge,
                                    bytes_transferred,
                                    total_bytes,
                                    chunks_verified,
                                    total_chunks,
                                    speed_bytes_per_sec,
                                    eta_seconds,
                                );
                            }
                            Ok(EngineEvent::TransferCompleted {
                                total_bytes,
                                total_chunks,
                                elapsed_ms,
                                average_speed_bytes_per_sec,
                                output_paths,
                                ..
                            }) => {
                                println!(
                                    "\n[{badge}] Verified {total_chunks}/{total_chunks} chunks ({}) in {:.2}s (avg {})",
                                    format_bytes(total_bytes),
                                    elapsed_ms as f64 / 1000.0,
                                    format_speed(average_speed_bytes_per_sec)
                                );
                                for p in output_paths {
                                    println!("  Saved: {p}");
                                }
                                break;
                            }
                            Ok(EngineEvent::TransferFailed { error, .. }) => {
                                engine.shutdown();
                                return Err(error.into());
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                            _ => {}
                        }
                    }
                }
            }

            engine.shutdown();
            Ok(())
        }
        Command::Peers {
            timeout_secs,
            device_name,
        } => {
            let engine = VeloceEngine::init(EngineConfigDto {
                device_name,
                bind_addr: None,
                enable_mdns: Some(true),
                parallel_streams: Some(4),
                ..Default::default()
            })
            .await?;

            println!("Scanning LAN (_veloce._udp.local.) for {timeout_secs}s...");
            tokio::time::sleep(Duration::from_secs(timeout_secs)).await;

            let peers = engine.list_local_peers();
            if peers.is_empty() {
                println!("No nearby Veloce peers found.");
            } else {
                println!("{:<12} | {:<24} | Addresses", "Short ID", "Device Name");
                println!("{:-<12}-+-{:-<24}-+-{:-<30}", "", "", "");
                for peer in peers {
                    println!(
                        "{:<12} | {:<24} | {}",
                        peer.short_id,
                        peer.device_name,
                        peer.addresses.join(", ")
                    );
                }
            }
            engine.shutdown();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_send_receive_and_peers_commands() {
        let send = parse_cli(
            [
                "send",
                "file1.bin",
                "dir/",
                "--name",
                "Box",
                "--relay",
                "1.2.3.4:7878",
                "--streams",
                "12",
                "--no-qr",
                "--once",
            ]
            .into_iter()
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            send,
            Command::Send {
                paths: vec!["file1.bin".into(), "dir/".into()],
                device_name: "Box".into(),
                bind_addr: None,
                relays: vec!["1.2.3.4:7878".into()],
                streams: 12,
                no_qr: true,
                once: true,
            }
        );

        let recv = parse_cli(
            [
                "receive",
                "veloce1abc",
                "-o",
                "/tmp/out",
                "--force-relay",
                "--streams",
                "6",
            ]
            .into_iter()
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            recv,
            Command::Receive {
                ticket: "veloce1abc".into(),
                output_dir: PathBuf::from("/tmp/out"),
                device_name: default_device_name(),
                relays: vec![],
                streams: 6,
                force_relay: true,
            }
        );

        assert!(parse_cli(["send"].into_iter().map(String::from)).is_err());
        assert!(parse_cli(["receive"].into_iter().map(String::from)).is_err());
    }
}
