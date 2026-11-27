<p align="center">
  <img src="assets/brand/veloce-logo.svg" alt="Veloce" width="96" height="96" />
</p>

<h1 align="center">Veloce</h1>

<p align="center">
  <strong>High-performance cross-platform local &amp; global P2P file transfer built in Rust.</strong>
</p>

Veloce combines zero-configuration LAN transfers (mDNS auto-discovery + explicit approval) with ephemeral, torrent-style global transfers powered by **QUIC (BBR)**, **BLAKE3 Merkle chunking**, **UDP hole-punching**, and a **blind end-to-end encrypted relay** fallback. One pure-Rust core powers the Electron desktop app, Flutter Android/iOS mobile app, standalone CLI (`veloce`), and stateless UDP relay (`veloce-relay`).

---

## Architecture

```text
+-----------------------+   +---------------------------+   +------------------+
|  Desktop UI (/ui)     |   |  Mobile App (/mobile)     |   |  CLI (/cli)      |
|  Electron + React     |   |  Flutter (Android & iOS)  |   |  veloce binary   |
+-----------+-----------+   +-------------+-------------+   +--------+---------+
            | napi-rs (.node)             | flutter_rust_bridge v2   | direct Rust
            v                             v                          v
+------------------------------------------------------------------------------+
|                       VeloceEngine (core-ffi / core-mobile-ffi)              |
+------------------------------------------------------------------------------+
|  core-protocol   | Postcard wire frames, handshake FSM, traversal-safe       |
|                  | manifests, veloce1... session tickets, TransferProfile    |
+------------------+-----------------------------------------------------------+
|  core-storage    | Dynamic 1–4 MiB BLAKE3 Merkle chunking, buffered/mmap     |
|                  | I/O, chunk dedup, positional pwrite, atomic resume states |
+------------------+-----------------------------------------------------------+
|  core-transport  | quinn QUIC + BBR, ephemeral Ed25519 mTLS 1.3, mDNS        |
|                  | discovery, NAT hole-punching, live path upgrade           |
+------------------------------------------------------------------------------+
                                     |
                                     v
                    +---------------------------------+
                    |         MagicUdpSocket          |
                    |  Single UDP port demultiplexer  |
                    |  RFC 9000 bit 6 = 1 -> QUIC     |
                    |  0x1BVLC (bit 6 = 0) -> Signal  |
                    +----+-----------+-----------+----+
                         |           |           |
                         v           v           v
                       [LAN]   [Direct-P2P]  [veloce-relay]
                      (mDNS)   (UDP Punch)   (Blind E2EE UDP)
```

### Wire & Transport Design

- **Multi-Stream QUIC with BBR Congestion Control:** Uses `quinn` explicitly tuned with `quinn::congestion::BbrConfig`, 512 KiB initial congestion window, 1380 B safe MTU (leaving headroom for relay encapsulation inside 1500 B Ethernet), and 64 MiB connection / 16 MiB stream flow-control windows.
- **Pipelined Zero-Extra-Copy Streaming:** Each QUIC stream pipelines multiple chunk requests simultaneously (`Desktop`: 8 streams, 4-deep pipeline, 1–4 MiB chunks, 24 concurrent disk writes; `Mobile`: 4 streams, 2-deep pipeline, 1 MiB chunk ceiling, 6 concurrent disk writes, buffered I/O bypassing `memmap2` to prevent iOS Jetsam OOM kills).
- **Single-Port QUIC + Signaling Multiplexing (`MagicUdpSocket`):** Uses RFC 9000's fixed bit (`0x40`) to multiplex QUIC packets and `0x1BVLC` signaling packets (STUN reflexive discovery, peer lookup, simultaneous UDP hole-punching, and blind `RelayForward` datagrams) on a single UDP socket. Supports kernel UDP GSO/GRO (`UdpSocketState`) with graceful fallback when mobile kernels restrict `UDP_SEGMENT` or large `SO_RCVBUF`/`SO_SNDBUF` allocations.
- **Live `Relayed -> Direct-P2P` Upgrade:** Global transfers start immediately over an encrypted relay tunnel if NATs block initial direct probes, then upgrade seamlessly to direct UDP hole-punched QUIC without dropping the transfer or re-sending verified chunks.
- **Pause & Instant Resume:** Every chunk is verified against the session's BLAKE3 Merkle root before writing via positional `pwrite`/`seek_write` into `.part` files. Verified progress persists atomically in `.veloce-resume-*.state` bitfields; pausing, cancelling, or mobile backgrounding resumes cleanly from the exact remaining chunks.

---

## Security Model

See [SECURITY.md](SECURITY.md) for the complete threat model and audit details.

- **Ephemeral Ed25519 mTLS 1.3:** Every node generates an ephemeral Ed25519 identity keypair embedded in a self-signed X.509 certificate. Custom `rustls` certificate verifiers cryptographically bind `NodeId == Ed25519 public key` on both client and server sides, defeating MitM attacks even across untrusted relays.
- **BLAKE3 Merkle Integrity:** Every chunk includes a domain-separated, chunk-index-bound Merkle inclusion proof verified against the session ticket's root hash prior to disk commitment.
- **Blind End-to-End Encrypted Relay:** `veloce-relay` never sees plaintext files, filenames, or TLS keys — it only forwards opaque QUIC/TLS 1.3 ciphertext packets and enforces per-IP token-bucket packet/bandwidth rate limits, anti-amplification control limits, strict 2048 B datagram caps, and peer registration caps.
- **Path Traversal Defense:** Receiver manifest validation rejects absolute paths, `..` components, NUL bytes, Windows drive prefixes, and symlinks outside the output directory.
- **Single-`unsafe` Audit:** The entire 8-crate Rust workspace contains strictly **1** `unsafe` call (`memmap2::Mmap::map` read-only mapping in `core/core-storage/src/chunker.rs`, bypassed automatically under `TransferProfile::Mobile`).

---

## Quick Start

### 1. Desktop GUI (Electron + React + `napi-rs`)

Download a prebuilt Linux (`AppImage` / `.deb`) or Windows (`NSIS` / `Portable`) release from [GitHub Releases](https://github.com/Niloyyy/Veloce/releases), or build from source:

```bash
cd ui
npm ci
npm run build:native
npm run test:native
npm run typecheck
npm run build
npm start
```

Configure device name, default download folder, DNS/IP relay hosts (`host:port`), and parallel QUIC streams directly inside the **Settings** modal (persisted to `veloce-settings.json` in Electron's `userData` directory).

### 2. Mobile App (Flutter — Android & iOS)

Download the latest CI-built Android APK from the **Mobile & Core CI** workflow artifacts (or GitHub Releases), or build locally:

```bash
# 1. Regenerate flutter_rust_bridge v2 bindings
cd mobile && flutter pub get && cd ..
flutter_rust_bridge_codegen generate

# 2. Android (arm64-v8a, 16 KB ELF page-aligned for Android 15+)
bash mobile/scripts/build-android-rust.sh release
cd mobile && flutter build apk --release --target-platform android-arm64 --tree-shake-icons --split-debug-info=build/symbols --obfuscate

# 3. iOS (aarch64-apple-ios static XCFramework linked with symbol preservation)
bash mobile/scripts/build-ios-rust.sh
cd mobile && flutter build ios --release --no-codesign
```

- **Android:** Acquires `WifiManager.MulticastLock` for mDNS, queries `ConnectivityManager` for API 30+ Wi-Fi LAN IPs, runs a `dataSync` Foreground Service + partial WakeLock during active transfers, and exports completed files out of app sandbox into public `Downloads/Veloce` via `MediaStore.Downloads`.
- **iOS:** Declares `NSLocalNetworkUsageDescription` + `_veloce._udp` Bonjour service, force-loads `VeloceCore.xcframework` with global C-ABI symbol preservation, exposes received files in the iOS Files app (`Documents/Veloce`), and auto-pauses active transfers on `AppLifecycleState.paused`.

### 3. Command-Line Interface (`veloce`)

```bash
cargo build --release -p veloce-cli

# Send files or folders (prints veloce1... ticket + terminal QR code + advertises on LAN)
./target/release/veloce send ./video.mp4 --relay relay.example.com:7878

# Receive via global ticket or QR URI
./target/release/veloce receive veloce1... -o ./downloads --streams 8

# Discover nearby LAN peers over mDNS
./target/release/veloce peers --timeout 5
```

### 4. Self-Hosting `veloce-relay`

`veloce-relay` is a single stateless UDP binary requiring zero database, TLS certificates, or disk storage:

```bash
cargo build --release -p veloce-relay

./target/release/veloce-relay \
  --bind 0.0.0.0:7878 \
  --max-peers 10000 \
  --max-peers-per-ip 16 \
  --max-mbps-per-ip 64
```

Open UDP port `7878` on your firewall and point Desktop Settings, Mobile Settings, or `veloce --relay` to `your-domain.example.com:7878`.

---

## Benchmarks

Command (non-root empirical impairment benchmark in `bench/`):

```bash
cargo run --release -p veloce-bench -- \
  --lan-size-mb 64 --wan-size-mb 8 --wan-tcp-max-secs 12 \
  --rtt-ms 100 --loss-pct 1.0 --streams 8 --chunk-kb 1024
```

### Verified Results (Userspace Impairment Harness)

| Scenario | Transport | Total Throughput | Steady-State Throughput | Notes |
| :--- | :--- | ---: | ---: | :--- |
| **Clean LAN Loopback** (256 MiB, 0 ms RTT, 0% loss) | **Veloce LAN mTLS 1.3 + BLAKE3** | **524.52 MB/s** (`0.49 s`) | **810.13 MB/s** (~6.5 Gbps) | Full Ed25519 mTLS 1.3 + parallel BLAKE3 (`19 ms`) + disk write & `fsync` |
| **Clean LAN Loopback** (256 MiB, 0 ms RTT, 0% loss) | Single-Stream TCP (Kernel) | 1,754.22 MB/s | 1,754.99 MB/s | Unencrypted memory-to-memory kernel loopback baseline |
| **Impaired WAN LFN** (16 MiB, 100 ms RTT, 1% loss) | **Veloce QUIC (BBR, 8 streams)** | **7.70 MB/s** | **14.34 MB/s** | Packet drops recovered seamlessly; full Merkle verified (**`71.13x`** faster than TCP) |
| **Impaired WAN LFN** (100 ms RTT, 1% loss) | Single-Stream TCP (Empirical Proxy) | 0.20 MB/s | 0.20 MB/s | 2.46 MiB in 12.0 s cap, 16 drops (HoL congestion stall) |

### Linux Kernel `tc netem` Benchmark (Root Mode)

To reproduce against real Linux kernel `netem` queueing disciplines (requires `sudo` + `iproute2`):

```bash
sudo ./bench/run_netem_bench.sh 64 50 0.5
```

| Scenario (`tc netem`) | Single-Stream TCP | Veloce QUIC (BBR, 8 streams) | Speedup |
| :--- | :---: | :---: | :---: |
| 64 MiB, 100 ms RTT (`50 ms` × 2), 1.0% loss (`0.5%` × 2) | _TBD (run on root Linux host)_ | _TBD (run on root Linux host)_ | _TBD_ |

---

## Development & Verification

```bash
# Rust lints (including optional napi-rs bridge) and full workspace test suite
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --features napi-addon -- -D warnings
cargo test --workspace

# Desktop Electron build & native addon smoke test
npm --prefix ui run build:native
npm --prefix ui run test:native
npm --prefix ui run typecheck
npm --prefix ui run build

# Flutter analysis & widget/controller tests
cd mobile && flutter analyze --no-fatal-infos && flutter test
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
