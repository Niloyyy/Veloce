# Security Policy & Cryptographic Architecture

## Cryptographic Guarantees

1. **Identity & Mutual Authentication (Ed25519 + mTLS 1.3)**
   - Every Veloce instance generates an ephemeral Ed25519 keypair at startup and embeds the public key into a self-signed X.509 certificate (`rcgen`).
   - A peer's `NodeId` is its 32-byte Ed25519 public key (encoded as Base32 in `veloce1...` tickets and mDNS TXT records).
   - Custom `rustls` `ServerCertVerifier` and `ClientCertVerifier` implementations extract the Ed25519 SubjectPublicKeyInfo from the peer certificate and require an exact match against the expected `NodeId`. Connections are terminated during the TLS 1.3 handshake if identities do not match.

2. **Chunk Integrity & Resume Safety (BLAKE3 Merkle Tree)**
   - Every transfer manifest is indexed into a binary BLAKE3 Merkle tree with domain-separated leaf hashes bound to `(chunk_index, chunk_bytes)` and internal node hashes.
   - Each received chunk includes its Merkle inclusion proof and is verified against the session ticket's 32-byte root hash **before** writing to `.part` files or updating `.veloce-resume-*.state` bitfields.
   - Corrupt, reordered, or forged chunks are rejected prior to disk persistence.

3. **Path Traversal & Filesystem Isolation**
   - `core-protocol` validates every manifest `relative_path`, rejecting empty segments, `.` / `..` traversal components, leading slashes, Windows drive prefixes, backslashes, NUL bytes, and control characters.
   - `core-storage` verifies all resolved destination paths stay strictly inside the user-selected output directory.

4. **Blind End-to-End Encrypted Relay (`veloce-relay`)**
   - All transfer traffic routed through `veloce-relay` remains encrypted inside peer-to-peer QUIC / TLS 1.3 packets. The relay cannot inspect, modify, or forge payloads, filenames, or session contents.
   - To prevent UDP amplification and memory exhaustion, `veloce-relay` enforces:
     - Strict `2,048 B` maximum UDP datagram length prior to deserialization and `1,600 B` maximum `RelayForward` payload.
     - Per-source-IP token-bucket packet (`4,000 pkt/s`, burst `8,000`) and bandwidth (`8 MiB/s`, burst `16 MiB` default, configurable via `--max-mbps-per-ip`) rate limiters.
     - Tighter per-IP control-plane bucket (`30 req/s`, burst `60`) for `StunRequest`, `RelayRegister`, `PeerLookup`, and `PunchNotify`.
     - Bounded peer table (`10,000` global peers, `16` peers per IP, `8` local addresses per peer, `65,536` tracked rate-limit IPs).

5. **Memory Safety (`unsafe` Audit)**
   - The entire 8-crate Rust workspace contains strictly **1** `unsafe` call: read-only `memmap2::Mmap::map` in `core/core-storage/src/chunker.rs` (documented with safety invariants, fallback to buffered I/O on failure, and bypassed completely under `TransferProfile::Mobile`).
   - All network deserialization (`postcard` wire frames, `veloce1...` tickets, `0x1BVLC` UDP signaling headers) enforces explicit upper bounds before allocation and is covered by adversarial boundary unit tests.

## Reporting a Vulnerability

If you discover a security vulnerability in Veloce (cryptographic, transport, parser DoS, path traversal, or FFI memory safety), please open a private security advisory on GitHub (`https://github.com/Niloyyy/Veloce/security/advisories/new`) with reproduction steps and affected versions rather than filing a public issue.
