# Changelog

All notable changes to Veloce are documented in this file.

## [0.1.1] - 2026-11-24

### Fixed
- **Global Session Tickets & NAT Traversal (`core-transport`, `core-ffi`):**
  - Stripped unspecified (`0.0.0.0` / `::`) bind addresses from generated `veloce1...` tickets so receivers never dial `0.0.0.0` or loop back to themselves (`PeerMismatch`).
  - Added built-in RFC 5389 STUN Binding discovery (`stun.cloudflare.com:3478`, `stun.l.google.com:19302`) multiplexed directly on `MagicUdpSocket` so global tickets discover public `IP:port` endpoints out of the box without requiring a custom relay for Cone NATs.
  - Added `host_session_async` to refresh relay registrations and STUN mappings dynamically when hosting a new session across Wi-Fi / cellular switches.
  - Wired `EndpointSessionOutcome` from `VeloceEndpoint::handle_incoming_connection` to `VeloceEngine` so hosted global sends and accepted incoming LAN offers reliably emit `TransferCompleted` / `TransferFailed` (releasing Android's Foreground Service / WakeLock and triggering `MediaStore.Downloads` public export on LAN receives).
- **Transfer History Management & Error UX (`/mobile`, `/ui`):**
  - Added per-transfer Delete controls, swipe-to-delete on Mobile, and "Clear History" actions on both Mobile and Desktop, plus persistent transfer history across app restarts.
  - Fixed Desktop cancel handling so cancelled transfers immediately transition to `"cancelled"` state, and added one-tap **Retry** on failed receives using saved `.veloce-resume-*.state` bitfields.
  - Added active ticket revocation (**Stop** button), dismissible error banners for FFI / NAT errors on Mobile & Desktop, ticket preservation on failed joins, Paste/Clear + QR Torch/Camera controls, cloud-backed file picker fallbacks, and Android multi-network + Mobile Hotspot (`ap*`, `wlan*`, `rndis*`) LAN IP discovery.

## [0.1.0] - 2026-11-24

### Added
- **Rust P2P Core (`core-protocol`, `core-storage`, `core-transport`, `core-ffi`, `core-mobile-ffi`):**
  - Multi-stream QUIC transport powered by `quinn` with BBR congestion control, 1380 B safe MTU, pipelined chunk transfers, and kernel UDP GSO/GRO with graceful mobile fallbacks.
  - Ephemeral Ed25519 mutual TLS 1.3 identity verification binding `NodeId` directly to X.509 peer certificates.
  - Dynamic 1–4 MiB BLAKE3 Merkle chunking, deduplication, traversal-safe manifests, positional `.part` writes, atomic `.veloce-resume-*.state` bitfields, and end-to-end Pause & Resume.
  - Single-port `MagicUdpSocket` multiplexing QUIC packets with `0x1BVLC` STUN, simultaneous UDP hole-punching, blind encrypted `RelayForward`, and live `Relayed -> Direct-P2P` connection upgrades.
  - Zero-configuration LAN mDNS discovery (`_veloce._udp.local.`) with physical NIC prioritization, Android API 30+ `getifs` + `ConnectivityManager` LAN IP overrides, and iOS `getifaddrs` support.
  - Configurable `TransferProfile` (`Desktop` vs bounded-memory `Mobile` bypassing `memmap2`) and DNS `host:port` relay resolution.
- **Desktop Client (`/ui`):**
  - Electron + React + TypeScript UI bridged directly to Rust via `napi-rs` (`veloce_core.node`).
  - Send & Receive views with drag-and-drop, QR tickets, nearby LAN peer push, incoming transfer approval modal, live MB/s sparklines, connection badges, Pause/Resume/Cancel controls, dark/light themes, and persistent Settings modal (`userData/veloce-settings.json`).
  - `electron-builder` packaging for Linux (`AppImage`, `.deb`) and Windows (`NSIS`, `Portable`).
- **Mobile Client (`/mobile`):**
  - Flutter (Material 3) Android & iOS client bridged to Rust via `flutter_rust_bridge` v2.13 (`core-mobile-ffi`).
  - File/media picker, QR code generation & camera scanner (`mobile_scanner`), nearby LAN peers, incoming offer sheet, live transfers, and Settings sheet.
  - Android 16 KB ELF page-aligned native libraries (`arm64-v8a`, `x86_64`), `MulticastLock`, Foreground Service + WakeLock, and `MediaStore.Downloads` public export.
  - iOS `VeloceCore.xcframework` static library linking with global symbol preservation, Files app integration, and lifecycle auto-pause.
- **CLI & Stateless Relay (`veloce`, `veloce-relay`):**
  - `veloce send`, `veloce receive`, and `veloce peers` CLI with terminal Unicode QR codes and live telemetry.
  - Stateless UDP `veloce-relay` with per-IP token-bucket packet/bandwidth rate limits, anti-amplification control limits, datagram caps, and peer registration caps.
- **Benchmarking, Security & CI/CD:**
  - `veloce-bench` comparing single-stream TCP vs multi-stream QUIC (BBR) under clean LAN and impaired WAN (userspace proxies + root `tc netem` harness).
  - Full security documentation (`SECURITY.md`) with a single-`unsafe` workspace audit.
  - Automated CI (`mobile.yml`) and multi-platform tag release workflow (`release.yml`) with `SHA256SUMS.txt`.
