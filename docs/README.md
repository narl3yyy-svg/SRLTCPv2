# SRLTCP v0.2.0

**Secure Reliable LAN/TCP/Serial P2P Messaging**

SRLTCP is a bleeding-edge, security-critical peer-to-peer messaging application designed for environments where reliability and confidentiality matter most — including noisy serial links, local networks, and wide-area connections.

## Quick Start

```bash
git clone https://github.com/narl3yyy-svg/SRLTCPv2.git
cd SRLTCPv2
./run.sh          # Linux/macOS
# or
run.bat           # Windows
```

See the root [README.md](../README.md) for the full quick-start guide.

That's it. The script installs Rust if needed, builds on first run, and launches the desktop app.

Press **Ctrl+C** for graceful shutdown. Run `./cleanup.sh` for a full reset.

## Features (v0.2.0)

| Feature | Description |
|---------|-------------|
| **Serial P2P** | COBS + CRC32 framing with ACK/NACK reliability |
| **LAN/WAN** | QUIC transport with encrypted fallback relay |
| **Post-Quantum** | Hybrid X25519 + ML-KEM-768 key exchange |
| **Messaging** | Double Ratchet E2EE with emoji support |
| **File Transfer** | Resumable chunked transfer on all transports |
| **Media** | Images and videos displayed inline in chat |
| **Calling** | WebRTC voice/video with E2EE signaling |
| **Discovery** | QR code + Short Authentication String (SAS) |
| **Android** | Foreground Service for always-on background P2P |

## Project Structure

```
SRLTCPv2/
├── run.sh / run.bat       # Download and run
├── cleanup.sh / .bat      # Full process cleanup
├── core/                  # Rust library + UniFFI
├── desktop/               # Tauri v2 desktop app
├── android/               # Kotlin + Jetpack Compose
└── docs/                  # Documentation
```

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — System design overview
- [SECURITY.md](SECURITY.md) — Threat model and security properties
- [CRYPTO.md](CRYPTO.md) — Cryptographic primitives and protocols
- [SERIAL_PROTOCOL.md](SERIAL_PROTOCOL.md) — Serial framing and reliability
- [BUILD.md](BUILD.md) — Build instructions for all platforms
- [USER_GUIDE.md](USER_GUIDE.md) — End-user guide

## License

MIT OR Apache-2.0