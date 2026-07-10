# SRLTCP v0.2.22

**Secure Reliable LAN/iroh/Serial P2P Messaging**

SRLTCP is a privacy-first peer-to-peer messaging application for noisy serial links, local networks, and wide-area connections via **iroh** NAT traversal (no port forwarding).

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

## Features (v0.2.22)

| Feature | Description |
|---------|-------------|
| **Serial P2P** | COBS + CRC32 framing with ACK/NACK reliability |
| **LAN/WAN** | iroh transport — relay + hole punching, no router config |
| **Post-Quantum** | Hybrid X25519 + ML-KEM-768 key exchange |
| **Messaging** | Double Ratchet E2EE with emoji support |
| **File Transfer** | Resumable chunked transfer with MB/s progress; save path in Settings |
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

| Document | Description |
|----------|-------------|
| [USER_GUIDE.md](USER_GUIDE.md) | End-user guide |
| [BUILD.md](BUILD.md) | Build from source |
| [ARCHITECTURE.md](ARCHITECTURE.md) | System design |
| [CRYPTO.md](CRYPTO.md) | Cryptographic protocol |
| [SECURITY.md](SECURITY.md) | Threat model and audit status |