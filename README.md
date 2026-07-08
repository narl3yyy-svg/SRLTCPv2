# SRLTCP

**Secure, reliable peer-to-peer messaging over LAN, WAN, QUIC, and serial.**

SRLTCP is privacy-first communication software: no accounts, no central servers, and end-to-end encryption with a human-verifiable SAS step before you trust a peer. A single Rust core powers the desktop (Tauri) and Android (Kotlin/Compose) clients, so crypto and protocol behavior stay consistent everywhere.

**Current release: [v0.2.11](https://github.com/narl3yyy-svg/SRLTCPv2/releases/tag/v0.2.11)**

---

## Security status (read this)

v0.2.11 fixes peer routing (`peer:{pubkey}` IDs), file chunk transfer, and trusted-peer auto-reconnect. v0.2.10 wired the hybrid handshake and Double Ratchet into the live message path. SAS codes are derived from a **canonical transcript** that both peers build identically — fix for the v0.2.9 mismatch bug.

**What works today**

- Wire handshake (X25519 + ML-KEM-768) with Ed25519-signed frames over QUIC/serial
- E2EE messaging after you confirm the matching SAS code
- Explicit trust gate — no plaintext chat until SAS is verified

**Caveats**

- QUIC uses ephemeral TLS certificates; long-term identity is bound at the application handshake layer, not in TLS
- Double Ratchet is a simplified implementation — not a full Signal-protocol clone
- WAN requires port forwarding (UDP/TCP 9473) on your router
- WebRTC calls and folder-transfer UI are not yet fully E2EE-wrapped

Do not treat this as production-grade secure messaging until you have reviewed [docs/SECURITY.md](docs/SECURITY.md) and [docs/CRYPTO.md](docs/CRYPTO.md) yourself.

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

`run.sh` downloads the matching **prebuilt binary** from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases) — no compiler required. If CI is still publishing after a fresh pull, the launcher retries automatically (up to ~3 minutes). Use `--rebuild` only when developing from source.

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
adb install dist/SRLTCPv2-0.2.10.apk
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
4. **Trust** only when both sides show the **same** code — then messaging is E2EE.

**WAN**: If you are not on the same LAN, set a WAN endpoint (`public.host:9473`) in Settings on both sides. Connect & Verify tries the LAN address from the QR first, then falls back to your WAN endpoint. Forward port 9473 on your router.

Saved contacts persist across restarts. Remove a contact on any platform to revoke trust and disconnect.

---

## Features

| Feature | Desktop | Android |
|---------|:-------:|:-------:|
| E2EE messaging | ✓ | ✓ |
| QUIC P2P (QR discovery) | ✓ | ✓ |
| WAN fallback connect | ✓ | ✓ |
| USB / serial transport | ✓ | — |
| File transfer + progress | ✓ | ✓ |
| Saved contacts & settings | ✓ | ✓ |
| Display name after auth | ✓ | ✓ |
| Voice / video calls | ✓* | ✓* |
| Foreground background service | — | ✓ |

\*Voice and video are experimental on some platforms.

### v0.2.10 highlights

- **SAS codes match** — canonical handshake transcript fix
- **WAN endpoint** in Settings (desktop + Android)
- Honest security documentation

### v0.2.9 highlights

- Wire handshake + Double Ratchet E2EE on the message path
- Visual QR codes on desktop and Android
- Prebuilt-first `run.sh`

See [CHANGELOG.md](CHANGELOG.md) and [docs/CRYPTO.md](docs/CRYPTO.md).

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
git tag -a v0.2.10 -m "SRLTCP v0.2.10"
git push origin main
git push origin v0.2.10
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