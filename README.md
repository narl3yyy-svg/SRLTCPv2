# SRLTCP

**Secure, reliable peer-to-peer messaging over iroh (NAT traversal), LAN, and serial.**

SRLTCP is privacy-first communication software: no accounts, no central servers, and end-to-end encryption with a human-verifiable SAS step before you trust a peer. A single Rust core powers the desktop (Tauri) and Android (Kotlin/Compose) clients.

**Current release: [v0.3.1](https://github.com/narl3yyy-svg/SRLTCPv2/releases/tag/v0.3.1)**

---

## Security status (read this)

v0.3.1 focuses on **production hardening**:

- **Persistent long-term identity** on desktop and Android (contacts survive restarts)
- Secret **zeroization**, QR-refresh identity binding, serial receive-buffer DoS caps
- **Release size optimizations** (LTO, strip, arm64-slim Android APK)
- Honest residual-risk documentation

**What works today**

- Wire handshake (X25519 + ML-KEM-768) with Ed25519-signed frames over iroh/serial
- Signal-spec Double Ratchet E2EE after SAS confirmation
- iroh NAT traversal — connect across networks without router config
- Explicit trust gate — no plaintext chat until SAS is verified
- Stable identity across app restarts

**Caveats**

- `double-ratchet-2` is still a **pre-release** crate — see [docs/SECURITY.md](docs/SECURITY.md)
- WebRTC media uses STUN/DTLS-SRTP; **call audio/video is not Double-Ratchet E2EE**
- Chat history at rest is not encrypted beyond OS file permissions
- Not independently audited — suitable for personal / trusted-circle use

---

## Quick start (no compiler)

### Desktop (Linux / macOS)

```bash
git clone https://github.com/narl3yyy-svg/SRLTCPv2.git
cd SRLTCPv2
./run.sh
```

Update later:

```bash
git pull && ./run.sh --pull
```

`run.sh` downloads the matching **prebuilt binary** from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases). Use `--rebuild` only when developing from source.

| Flag | Purpose |
|------|---------|
| `--pull` | `git pull --ff-only` from `origin/main` before launch |
| `--rebuild` | Compile from source, then run |
| `--no-prebuilt` | Use only local `dist/` binaries |

### Windows

```bat
git clone https://github.com/narl3yyy-svg/SRLTCPv2.git
cd SRLTCPv2
run.bat
```

### Android

Install the APK from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases/latest) (default is **arm64-v8a**, modern phones):

```bash
adb install dist/SRLTCPv2-0.3.1.apk
```

Or build locally (JDK 17, Android SDK/NDK):

```bash
./scripts/build-android.sh
# Multi-ABI universal APK:
SRLTCP_UNIVERSAL_APK=1 ./scripts/build-android.sh
```

---

## Connect securely (QR + SAS)

1. **Share** your QR code (desktop sidebar or Android connect sheet).
2. **Paste** the peer's QR payload and tap **Connect & Verify**.
3. **Compare** the 6-digit SAS code out-of-band (voice, in person, etc.).
4. **Trust** only when both sides show the **same** code — then messaging is E2EE.

Saved verified contacts persist with per-peer chat history. On startup the app auto-reconnects to your last active peer when the stored identity matches. Your own Ed25519 identity is stored securely on-device so peers keep recognizing you after restart.

---

## Features

| Feature | Desktop | Android |
|---------|:-------:|:-------:|
| E2EE messaging | ✓ | ✓ |
| Persistent identity | ✓ | ✓ |
| iroh P2P (QR v4 + NAT) | ✓ | ✓ |
| Offline message queue | ✓ | ✓ |
| USB / serial transport | ✓ | — |
| File transfer + progress | ✓ | ✓ |
| Saved contacts & settings | ✓ | ✓ |
| Voice / video calls (WebRTC) | ✓* | ✓* |
| Foreground background service | — | ✓ |

\* Call **signaling** is E2EE; **media** is DTLS-SRTP only.

---

## Project layout

```
SRLTCPv2/
├── run.sh / run.bat              # Launch desktop (prebuilt auto-download)
├── scripts/                      # build-desktop, build-android, release helpers
├── core/                         # Rust: crypto, iroh, serial, UniFFI
├── desktop/                      # Tauri v2 UI
├── android/                      # Kotlin + Compose
├── docs/                         # Architecture, crypto, security, user guide
└── dist/                         # APK + prebuilt binaries (local/CI)
```

---

## Building from source

See [docs/BUILD.md](docs/BUILD.md). Desktop needs Rust 1.85+ and platform libraries (e.g. webkit2gtk on Linux).

```bash
./run.sh --rebuild
./scripts/build-desktop.sh
./scripts/build-android.sh
cargo test -p srltcp-core
```

---

## Releases

Pushing a version tag triggers CI to publish desktop prebuilts and the Android APK:

```bash
# Bump version in Cargo.toml + android versionName/versionCode, then:
git tag -a v0.3.1 -m "SRLTCP v0.3.1"
git push origin main
git push origin v0.3.1
```

---

## Documentation

- [docs/USER_GUIDE.md](docs/USER_GUIDE.md) — End-user guide  
- [docs/BUILD.md](docs/BUILD.md) — Build instructions  
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design  
- [docs/SECURITY.md](docs/SECURITY.md) — Threat model & residual risks  
- [docs/CRYPTO.md](docs/CRYPTO.md) — Cryptography details  
- [CHANGELOG.md](CHANGELOG.md) — Version history  

---

## License

MIT OR Apache-2.0 — see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
