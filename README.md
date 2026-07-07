# SRLTCPv2

**Secure Reliable LAN/TCP/Serial P2P Messaging — v0.2.2**

Rust core + Tauri desktop + Android foreground service. COBS/CRC serial, QUIC networking, hybrid post-quantum crypto, file transfer, and voice/video calls.

## Quick Start

### Desktop

```bash
git clone https://github.com/narl3yyy-svg/SRLTCPv2.git
cd SRLTCPv2
./run.sh          # Linux/macOS
run.bat           # Windows
```

Press **Ctrl+C** for graceful shutdown.

### Android

```bash
./scripts/build-android.sh          # Full build (native + APK + cleanup)
# or, if jniLibs already exist:
./scripts/assemble-apk.sh           # Gradle only

adb install dist/SRLTCPv2-0.2.2.apk
```

**Requirements:** Android NDK, Android SDK API 35, **JDK 17** (JDK 21+ will fail).

## Features

| Feature | Desktop | Android |
|---------|---------|---------|
| E2EE messaging | ✓ | ✓ |
| QUIC P2P | ✓ | ✓ |
| Serial transport | ✓ | — |
| File transfer + progress | ✓ | ✓ |
| Voice / video calls | ✓ | ✓ |
| Inline images / video | ✓ | ✓ |
| Background service | — | Foreground Service |

## Project Structure

```
SRLTCPv2/
├── run.sh / run.bat                 # Launch desktop
├── cleanup.sh                       # Stop processes, optional --android-build
├── scripts/
│   ├── build-android.sh             # Full Android pipeline
│   ├── assemble-apk.sh              # Gradle-only (needs jniLibs)
│   ├── cleanup-android-build.sh     # Remove Gradle caches
│   ├── create-github-release.sh     # Tag + release + APK
│   └── lib/android-env.sh           # JDK/SDK/NDK detection
├── core/                            # Rust library + UniFFI
├── desktop/                         # Tauri v2 app
├── android/                         # Kotlin + Compose
└── dist/                            # Built APK output
```

## Cleanup

```bash
./cleanup.sh                         # Stop desktop, release ports
./cleanup.sh --android-build         # Also remove Gradle caches
./scripts/cleanup-android-build.sh --full  # Also remove jniLibs
```

## Known Issues

| Issue | Workaround |
|-------|------------|
| Gradle fails with `26.0.1` | Use JDK 17: `export JAVA_HOME=/usr/lib/jvm/java-17-openjdk` |
| No jniLibs after clone | Run `./scripts/build-android.sh` (not just gradlew) |
| Desktop needs rebuild after pull | `cargo build --release -p srltcp-desktop` |

## Documentation

- [docs/BUILD.md](docs/BUILD.md) — Detailed build instructions
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design
- [docs/SECURITY.md](docs/SECURITY.md) — Threat model
- [docs/USER_GUIDE.md](docs/USER_GUIDE.md) — End-user guide

## License

MIT OR Apache-2.0