# SRLTCP

**Secure, reliable peer-to-peer messaging over LAN, QUIC, and serial.**

SRLTCP is privacy-first communication software: no accounts, no central servers, and end-to-end encryption with a human-verifiable SAS step before you trust a peer. A single Rust core powers the desktop (Tauri) and Android (Kotlin/Compose) clients, so crypto and protocol behavior stay consistent everywhere.

**Current release: [v0.2.8](https://github.com/narl3yyy-svg/SRLTCPv2/releases/tag/v0.2.8)**

---

## Why SRLTCP

| Principle | What it means |
|-----------|----------------|
| **Freedom to run** | Clone, build, or download prebuilt binaries — no vendor lock-in |
| **Privacy by design** | Hybrid key exchange, double ratchet, QR + SAS verification |
| **Works offline** | Serial/USB cable transport for air-gapped or low-power links |
| **Lightweight** | Rust core tuned for modest hardware (e.g. Raspberry Pi class devices) |

---

## Quick start

### Desktop (Linux / macOS)

```bash
git clone https://github.com/narl3yyy-svg/SRLTCPv2.git
cd SRLTCPv2
./run.sh
```

**Update an existing checkout** (no need to re-clone):

```bash
git pull && ./run.sh --pull
```

`run.sh` downloads the matching prebuilt binary from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases) when available, or builds from source as a fallback.

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

Install the APK from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases/latest):

```bash
adb install dist/SRLTCPv2-0.2.8.apk
```

Or build locally (JDK 17, Android SDK/NDK):

```bash
./scripts/build-android.sh
```

---

## Connect securely (QR + SAS)

1. **Share** your QR code (desktop sidebar or Android connect sheet).
2. **Paste** the peer's QR payload and tap **Connect & Verify**.
3. **Compare** the 6-digit SAS code out-of-band (voice, in person, etc.).
4. **Trust** only when both sides show the same code — then messaging is E2EE.

Saved contacts persist across restarts. Remove a contact on any platform to revoke trust and disconnect.

---

## Features

| Feature | Desktop | Android |
|---------|:-------:|:-------:|
| E2EE messaging | ✓ | ✓ |
| QUIC P2P (QR discovery) | ✓ | ✓ |
| USB / serial transport | ✓ | — |
| File transfer + progress | ✓ | ✓ |
| Saved contacts & settings | ✓ | ✓ |
| Display name after auth | ✓ | ✓ |
| Voice / video calls | ✓* | ✓* |
| Foreground background service | — | ✓ |

\*Voice and video are experimental on some platforms.

### v0.2.8 highlights

- Desktop SAS verification modal shows codes reliably (high-contrast UI)
- Android chat input rises with the keyboard; keyboard **Send** submits messages
- Disconnect button and saved peers sheet on Android
- Remove trusted contacts on desktop and Android
- USB serial ports show manufacturer/product names, not only `/dev/tty*`
- Modern QR and serial-picker styling
- `git pull && ./run.sh --pull` update workflow

---

## Project layout

```
SRLTCPv2/
├── run.sh / run.bat              # Launch desktop (prebuilt auto-download)
├── cleanup.sh                    # Stop processes, release ports
├── scripts/
│   ├── build-desktop.sh
│   ├── build-android.sh
│   └── lib/version.sh            # Version from Cargo.toml
├── core/                         # Rust: crypto, QUIC, serial, UniFFI
├── desktop/                      # Tauri v2 UI
├── android/                      # Kotlin + Compose
│   └── app/src/main/java/com/srltcp/v2/
│       ├── data/                 # Preferences, saved contacts
│       └── ui/                   # Settings, peers sheets
└── dist/                         # APK + prebuilt binaries
```

---

## Building from source

See [docs/BUILD.md](docs/BUILD.md). Desktop needs Rust 1.85+ and platform libraries (e.g. webkit2gtk on Linux).

```bash
./run.sh --rebuild
./scripts/build-desktop.sh
./scripts/build-android.sh
```

---

## Security

SRLTCP uses hybrid post-quantum key exchange, a double ratchet for forward secrecy, and SAS verification to mitigate MITM during first contact. See [docs/SECURITY.md](docs/SECURITY.md) and [docs/CRYPTO.md](docs/CRYPTO.md).

---

## Releases

Pushing a version tag triggers CI to publish desktop prebuilts and the Android APK:

```bash
git tag -a v0.2.8 -m "SRLTCP v0.2.8"
git push origin main
git push origin v0.2.8
```

---

## Documentation

- [docs/USER_GUIDE.md](docs/USER_GUIDE.md) — End-user guide
- [docs/BUILD.md](docs/BUILD.md) — Build instructions
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design
- [docs/SECURITY.md](docs/SECURITY.md) — Threat model

---

## License

MIT OR Apache-2.0