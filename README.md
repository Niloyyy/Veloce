# Veloce — Cross-Platform Local & Global P2P File Transfer

Veloce is a high-performance peer-to-peer (P2P) file transfer application built in **Rust** with a decoupled **Electron (TypeScript/React)** desktop frontend connected via **`napi-rs`**.

It combines LocalSend-style zero-configuration LAN transfers with ephemeral, torrent-style global transfers powered by **QUIC (BBR)**, **BLAKE3 Merkle chunking**, **UDP hole-punching**, and an **end-to-end encrypted stateless relay** fallback.

---

## Architecture

### Workspace Layout

```text
/core
  /core-protocol   Postcard wire protocol, handshake state machine, traversal-safe manifests, veloce1... session tickets
  /core-storage    Dynamic 1–4 MiB BLAKE3 Merkle chunking, mmap/positional I/O, deduplication, atomic resume bitfields
  /core-transport  Quinn QUIC (BBR), ephemeral Ed25519 mTLS 1.3, mDNS discovery, MagicUdpSocket, STUN/hole-punching & live path upgrades
  /core-ffi        Async napi-rs native addon + VeloceEngine command/event bridge (ThreadsafeFunction non-blocking stream)
/relay             Minimal stateless UDP STUN, simultaneous punch signaling, and blind encrypted packet relay (veloce-relay)
/ui                Electron + React + TypeScript UI (Send/Receive tabs, QR code, LAN peers, real-time telemetry, dark/light mode)
/bench             Single-stream TCP vs. multi-stream QUIC (BBR) benchmark under 100ms RTT & 1% packet loss
```

### Key Technical Guarantees

- **Zero Head-of-Line Blocking:** Up to 8–32 concurrent QUIC bidirectional streams request and deliver verified chunks out-of-order.
- **BBR Congestion Control:** Explicitly configured with `quinn::congestion::BbrConfig` and 64 MiB connection / 16 MiB stream flow-control windows for Long Fat Networks.
- **Mutual TLS 1.3 (Ed25519 Node IDs):** Every peer generates an ephemeral Ed25519 keypair embedded in a self-signed X.509 certificate. Custom `rustls` verifiers cryptographically bind `NodeId = Ed25519 public key`.
- **BLAKE3 Merkle Tree Integrity & Instant Resume:** Every chunk carries a domain-separated, index-bound Merkle inclusion proof verified before committing to `.part` files via positional writes (`pwrite` / `seek_write`). Progress persists atomically in `.veloce-resume-*.state` bitfields.
- **Single-Port QUIC + Signaling Multiplexing (`MagicUdpSocket`):** Uses RFC 9000 fixed-bit demultiplexing (`0x1BVLC`, bit 6 = 0) so LAN QUIC, STUN, UDP hole-punching, and blind encrypted `RelayForward` share one UDP port with live `Relayed -> Direct-P2P` upgrades without dropping active transfers.

---

## Building, Testing & Running

### 1. Run the Full Rust Test Suite

```bash
cargo test --workspace
```

### 2. Build & Smoke-Test the Native `napi-rs` Bridge

```bash
cd ui
npm install
npm run build:native
npm run test:native
```

### 3. Typecheck, Build & Launch the Electron UI

```bash
cd ui
npm run typecheck
npm run build
npm start
```

### 4. Run the Stateless Global Relay Server (Optional Self-Hosting)

```bash
cargo run --release -p veloce-relay -- --bind 0.0.0.0:7878
# Point Electron clients to your relay:
VELOCE_RELAY_ADDRS="your-relay.example.com:7878" npm --prefix ui start
```

### 5. Run the TCP vs. Multi-Stream QUIC (BBR) Benchmark

Non-root userspace impairment mode (100ms RTT, 1% packet loss):

```bash
cargo run --release -p veloce-bench -- --size-mb 8 --rtt-ms 100 --loss-pct 1.0 --streams 8
```

Kernel-level Linux `tc netem` mode (when running as root):

```bash
sudo ./bench/run_netem_bench.sh 16 50 0.5
```
