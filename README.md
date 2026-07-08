# SRLTCPv2

**Secure Reliable LAN/TCP/Serial P2P Messaging — v0.2.6**

Rust core + Tauri desktop + Android foreground service. COBS/CRC serial, QUIC networking, hybrid post-quantum crypto, file transfer, and voice/video calls.

## Quick Start (Prebuilt — No Compiler Required)

Clone the repo and run the launcher. It downloads the matching prebuilt binary from [GitHub Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases) — no Rust or build tools needed.

### Desktop

```bash
git clone https://github.com/narl3yyy-svg/SRLTCPv2.git
cd SRLTCPv2
./run.sh          # Linux/macOS
```

```bat
git clone https://github.com/narl3yyy-svg/SRLTCPv2.git
cd SRLTCPv2
run.bat           # Windows
```

Press **Ctrl+C** or close the window for graceful shutdown. Use `--rebuild` only if you want to compile from source.

| Platform | Prebuilt asset (auto-downloaded) |
|----------|----------------------------------|
| Linux x86_64 | `srltcp-desktop-linux-x86_64` |
| macOS Apple Silicon | `srltcp-desktop-macos-aarch64` |
| macOS Intel | `srltcp-desktop-macos-x86_64` |
| Windows x86_64 | `srltcp-desktop-windows-x86_64.exe` |

**Flags:** `--rebuild` compiles from source; `--no-prebuilt` skips GitHub download (uses local `dist/` only).

### Android

Download `SRLTCPv2-0.2.6.apk` from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases/latest) and install:

```bash
adb install SRLTCPv2-0.2.6.apk
```

Or build from source (requires NDK, SDK, JDK 17):

```bash
./scripts/build-android.sh
```

## Connecting Peers (QR + SAS)

v0.2.6 uses **QR-only** peer discovery — no manual IP entry. QR codes embed the peer's LAN address for automatic connection:

1. Share your QR code (desktop sidebar or Android connect sheet)
2. Paste the peer's QR payload and click **Connect & Verify (QR + SAS)**
3. Compare the 6-digit SAS code out-of-band before trusting
4. Start messaging once codes match

## Building from Source (Developers)

See [docs/BUILD.md](docs/BUILD.md) for full instructions. Desktop requires Rust 1.85+ and platform libraries (webkit2gtk on Linux).

```bash
./run.sh --rebuild                    # Desktop (compile + launch)
./scripts/build-desktop.sh            # Desktop prebuilt only
./scripts/build-android.sh            # Android APK
```

## Features

| Feature | Desktop | Android |
|---------|---------|---------|
| E2EE messaging | ✓ | ✓ |
| QUIC P2P (QR discovery) | ✓ | ✓ |
| Serial transport | ✓ | — |
| File transfer + progress | ✓ | ✓ |
| Voice / video calls | ✓ | ✓ |
| Inline images / video | ✓ | ✓ |
| Background service | — | Foreground Service |

## Project Structure

```
SRLTCPv2/
├── run.sh / run.bat                 # Launch desktop (prebuilt auto-download)
├── cleanup.sh                       # Stop processes, optional --android-build
├── scripts/
│   ├── build-desktop.sh             # Build + stage desktop prebuilt
│   ├── build-android.sh             # Full Android pipeline
│   ├── assemble-apk.sh              # Gradle-only (needs jniLibs)
│   ├── create-github-release.sh     # Manual release fallback
│   └── lib/
│       ├── version.sh               # Read version from Cargo.toml
│       └── android-env.sh           # JDK/SDK/NDK detection
├── .github/workflows/release.yml    # CI: build all platforms on tag push
├── core/                            # Rust library + UniFFI
├── desktop/                         # Tauri v2 app
├── android/                         # Kotlin + Compose
└── dist/                            # Built APK + prebuilt binaries
```

## Releases

Every version tag (`v*`) triggers GitHub Actions to build and publish:

- Desktop prebuilts for Linux, macOS (Intel + Apple Silicon), and Windows
- Android APK (`SRLTCPv2-<version>.apk`)

```bash
git tag -a v0.2.6 -m "SRLTCP v0.2.6"
git push origin main
git push origin v0.2.6
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
| Prebuilt download fails | Check [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases) for your platform; use `--rebuild` as fallback |

## Documentation

- [docs/BUILD.md](docs/BUILD.md) — Detailed build instructions
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design
- [docs/SECURITY.md](docs/SECURITY.md) — Threat model
- [docs/USER_GUIDE.md](docs/USER_GUIDE.md) — End-user guide

## License

MIT OR Apache-2.0