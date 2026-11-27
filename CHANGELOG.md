# Changelog

All notable changes to Veloce are documented in this file.

## [0.1.4] - 2026-11-27

### Added
- **Kernel-Offloaded Ed25519 mTLS 1.3 LAN Data Lane (`810 MB/s` LAN Throughput):**
  - Added `LanTcpLane` multiplexed on the exact same port as `MagicUdpSocket`, pairing Ed25519 mutual TLS 1.3 (`TLS13_AES_128_GCM_SHA256` prioritized first) with kernel TCP TSO/GRO and `256 KiB` super-writes (`4` syscalls/MiB instead of `720`), boosting steady-state LAN throughput to **810.13 MB/s** (`524.52 MB/s` end-to-end with multi-core BLAKE3 indexing & `fsync`) while automatically falling back to multi-stream QUIC (BBR) (`14.34 MB/s` / `71.13x` TCP on `100 ms` RTT, `1%` loss WAN).
- **Automatic Cross-Session Missing-Chunk Deduplication & Auto-Reconnect:**
  - Interrupted or cancelled transfers now resume automatically even when the sender starts a **brand-new session** later for the same file(s), skipping all previously verified `.part` chunks as well as already-finalized destination files in multi-file batches, and reconnecting automatically on transient mid-transfer packet/network drops.
- **Sub-Second (`~1s`) Nearby LAN Device Sync:**
  - Paired mDNS with a `750 ms` UDP LAN presence beacon (`Hello`/`Goodbye`), dynamic Wi-Fi/cellular interface transition tracking, and active unicast `HolePunchProbe` liveness checks so devices joining Wi-Fi or switching to Mobile Data update Nearby Device lists within ~1 second.
- **Desktop System Tray Background Mode:**
  - Closing the desktop window keeps Veloce running in the system tray (configurable in Settings & tray menu) with single-instance focus lock, `powerSaveBlocker` during active transfers, silent background auto-accept for Trusted Devices, and automatic window wakeup/notification on untrusted offers.
- **Clean, User-Friendly UI, Live Theme Preview, & Per-File Unselect (`×`):**
  - Simplified Desktop and Mobile UIs with plain, jargon-free language, live theme preview on swatch tap/click, staged file selection with per-file `×` unselect + `+ Files` / `Clear`, dedicated **Send Anywhere (Create Share Code)** workflow, compact expandable text/link bar, and collapsible **Advanced Network Options**.
  - Added Android `WIFI_MODE_FULL_LOW_LATENCY` transfer locks and `<1 ms` zero-copy atomic `renameTo` export to `Downloads/Veloce` on Android 11+.

## [0.1.3] - 2026-11-27

### Added
- **Quick Text & Link Beam (`veloce-clip-message.txt`):**
  - Instant cross-platform text snippet and URL sharing (up to 256 KB) over both 1-click LAN peer push and Global QR / `veloce1...` tickets across Mobile and Desktop, auto-consumed into Copy / Open Link cards without cluttering `Downloads`.
- **Persistent Ed25519 Identity & Trusted Auto-Accept:**
  - Persistent Ed25519 identity keypairs (`PeerIdentity::load_or_generate`) across restarts with mTLS-verified `NodeId` trust pinning, zero-prompt auto-accept for trusted devices, per-peer shield toggles, `"Trust ..."` option on incoming offers, and Settings management.
- **Custom Save Folders & Open / Reveal Received Files:**
  - Persistent custom save folder picker on Mobile (Android SAF `ACTION_OPEN_DOCUMENT_TREE` + iOS folder picker) across Settings, Receive, and Incoming Offer sheets, plus one-tap **Open** and **Show in Folder** actions on completed received files across Desktop and Mobile.
- **Auto-Detected Hardware Model Names, 6 Curated Themes & Chunk-Start Timer:**
  - Auto-detected hardware/model names across Android (`Settings.Global.DEVICE_NAME` / `Build.MODEL`), iOS (`utsname.machine` mapping), Desktop (`DMI` / `ioreg` / BIOS registry), and CLI with one-click header rename and reset-to-model.
  - 6 curated themes (`Obsidian`, `Daylight`, `Midnight OLED`, `Nord Frost`, `Emerald`, `Sunset Ember`) and `OnceLock<Instant>` chunk-start elapsed transfer timer (`Took <duration>`) across Mobile and Desktop.

### Changed
- **85% Smaller Android APK & Optimized Binary / UI Bundles:**
  - Reduced the V2-signed Android release APK from **80.9 MB** (`v0.1.2`) down to **12.2 MB** (**6.6× smaller**) via DEFLATE native `.so` packaging (`useLegacyPackaging = true` while preserving 16 KB ELF segment alignment), unbundled Google Play Services MLKit barcode scanning, `arm64-v8a` phone targeting, R8 full-mode code/resource shrinking, and Dart AOT icon tree-shaking + debug-symbol splitting + obfuscation on Android & iOS.
  - Configured Cargo `[profile.release]` (`opt-level = "s"`, `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`) while keeping `opt-level = 3` on hot data-path crates (`blake3`, `core-storage`, `core-transport`, `quinn-proto`) and gating `serde_json` behind `napi-addon`, cutting `veloce` CLI by **32%** and `veloce-relay` by **29%**.
  - Stripped 54 unused Electron Chromium locales (`electronLanguages: [en-US]`) with maximum compression, dynamically code-split `qrcode` in Vite, and optimized Flutter & React UI rendering (`RepaintBoundary`, active-tab rendering, 50 ms / `requestAnimationFrame` progress coalescing, `React.memo`).

## [0.1.2] - 2026-11-26

### Added
- **Zero-Cost Global P2P Traversal (`core-transport`, `core-ffi`, Android):**
  - Dual-stack IPv6 + IPv4 single-port `MagicUdpSocket` (`[::]:0` with automatic `0.0.0.0:0` fallback) and dual-stack RFC 5389 STUN discovery, plus Android 4G/5G global IPv6 and mesh VPN (Tailscale / ZeroTier) ticket candidate support.
  - Pure-Rust, zero-`unsafe` UPnP IGD (SSDP + SOAP with LAN SSRF guard) and NAT-PMP (RFC 6886) automatic router UDP port forwarding with clean lease release on shutdown.
  - Happy-Eyeballs parallel direct-candidate racing (`JoinSet`) in `join_session` so unreachable private LAN IPs never stall public IPv6/UPnP/STUN dials.
  - Zero-cost `veloce-relay` deployment kit (`deploy/relay/`: Dockerfile, docker-compose, hardened systemd unit, Oracle Cloud Always Free guide), graceful `SIGTERM` handling, and optional compile-time `VELOCE_DEFAULT_RELAY` support.
- **Veloce "Swoop V" Brand Identity:**
  - Minimal two-tone soaring falcon logo wired across master SVGs (`assets/brand/`), Android mipmaps + API 26 adaptive icon + API 33 Material You monochrome icon, iOS opaque `AppIcon.appiconset`, Desktop Electron (`icon.png`, multi-res `icon.ico`, `favicon.svg`, `BrowserWindow` icon), Mobile/Desktop topbars, and `README.md`.
- **Hardened Persistent Android Release Signing:**
  - CI workflows and Gradle now fail fast on partial signing secrets and assert `apksigner` certificates are not `CN=Android Debug`, enabling seamless in-place Android APK upgrades from `v0.1.2` onward.

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
