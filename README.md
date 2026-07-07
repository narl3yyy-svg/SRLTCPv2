# SRLTCPv2

**Secure Reliable LAN/TCP/Serial P2P Messaging — v0.2.0**

SRLTCP is a security-focused peer-to-peer messaging stack with a Rust core, Tauri desktop app, and Android foreground-service client. It supports COBS+CRC serial links, QUIC networking, hybrid post-quantum crypto, file transfer, and voice/video calls.

## Quick Start

### Desktop (Linux / macOS / Windows)

```bash
git clone https://github.com/YOUR_USER/SRLTCPv2.git
cd SRLTCPv2
./run.sh          # Linux/macOS
# or
run.bat           # Windows
```

Press **Ctrl+C** for graceful shutdown (releases serial ports and QUIC listeners).

### Android

```bash
./scripts/build-android.sh
adb install dist/SRLTCPv2-debug.apk
```

Requires Android NDK, Android SDK API 35, and **JDK 17** for Gradle.

## Features

| Feature | Desktop | Android |
|---------|---------|---------|
| E2EE messaging | Yes | Yes |
| QUIC P2P | Yes | Yes |
| Serial transport | Yes | — |
| File transfer | Yes | Yes |
| Voice / video calls | Yes | Yes |
| Background service | — | Foreground Service |
| Inline images / video | Yes | Yes |

## Project Structure

```
SRLTCPv2/
├── run.sh / run.bat           # Build (if needed) and launch desktop
├── cleanup.sh / cleanup.bat   # Stop processes, release ports
├── scripts/
│   ├── build-android.sh       # Full Android build pipeline
│   └── cleanup-android-build.sh
├── core/                      # Rust library + UniFFI
├── desktop/                   # Tauri v2 desktop app
├── android/                   # Kotlin + Jetpack Compose
├── dist/                      # Built APK output (after android build)
└── docs/                      # Detailed documentation
```

## Documentation

- [docs/BUILD.md](docs/BUILD.md) — Build instructions for all platforms
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design
- [docs/SECURITY.md](docs/SECURITY.md) — Threat model
- [docs/USER_GUIDE.md](docs/USER_GUIDE.md) — End-user guide
- [docs/CRYPTO.md](docs/CRYPTO.md) — Cryptographic primitives
- [docs/SERIAL_PROTOCOL.md](docs/SERIAL_PROTOCOL.md) — Serial framing

## Cleanup

```bash
./cleanup.sh                  # Stop desktop app, release ports
./cleanup.sh --android-build  # Also remove Android Gradle caches
```

## License

MIT OR Apache-2.0