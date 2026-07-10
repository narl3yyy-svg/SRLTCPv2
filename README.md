# SRLTCP

**Secure, reliable peer-to-peer messaging over iroh (NAT traversal), LAN, and serial.**

SRLTCP is privacy-first communication software: no accounts, no central servers, and end-to-end encryption with a human-verifiable SAS step before you trust a peer. A single Rust core powers the desktop (Tauri) and Android (Kotlin/Compose) clients, so crypto and protocol behavior stay consistent everywhere.

**Current release: [v0.2.22](https://github.com/narl3yyy-svg/SRLTCPv2/releases/tag/v0.2.22)**

---

## Security status (read this)

v0.2.22 fixes **Linux voice/video calls** (WebKit WebRTC, portal permissions, GstIntRange) and **Android infinite spinner**. v0.2.21 adds **save-folder path**, **transfer MB/s**, **open file location**, and fixes **Android launch hang** (engine init off main thread). v0.2.20 fixes **SAS confirm / add-peer crash** (double-ratchet responder send chain). v0.2.19 fixes **macOS iroh DNS/relay connectivity** (router hijack) and **WebKit GStreamer video constraints**. v0.2.18 fixed **video playback controls** and **voice/video call reliability**. v0.2.17 fixed **call UI**, **peer presence**, **display names**, and **serial I/O**. v0.2.13+ uses **iroh** NAT traversal and **double-ratchet-2** E2EE with QR v4.

**What works today**

- Wire handshake (X25519 + ML-KEM-768) with Ed25519-signed frames over iroh/serial
- Signal-spec Double Ratchet E2EE after SAS confirmation
- iroh NAT traversal — connect across networks without router config
- Explicit trust gate — no plaintext chat until SAS is verified

**Caveats**

- iroh transport is encrypted separately from app-layer E2EE (defense in depth)
- WebRTC media uses STUN/DTLS-SRTP; call signaling is E2EE over iroh
- `ml-kem` hybrid KEX is not independently audited — see [docs/CRYPTO.md](docs/CRYPTO.md)

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
adb install dist/SRLTCPv2-0.2.22.apk
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

**Cross-network**: QR v4 includes an **iroh ticket** — peers connect through NAT relay/hole-punching with no router configuration.

Saved verified contacts reconnect automatically (fresh handshake, no SAS re-prompt unless identity changes). Messages sent while offline queue until reconnect.

---

## Features

| Feature | Desktop | Android |
|---------|:-------:|:-------:|
| E2EE messaging | ✓ | ✓ |
| iroh P2P (QR v4 + NAT) | ✓ | ✓ |
| Offline message queue | ✓ | ✓ |
| USB / serial transport | ✓ | — |
| File transfer + progress (MB/s) | ✓ | ✓ |
| Save folder path + open location | ✓ | ✓ |
| Saved contacts & settings | ✓ | ✓ |
| Display name after auth | ✓ | ✓ |
| Voice / video calls (WebRTC) | ✓ | ✓ |
| Foreground background service | — | ✓ |

### v0.2.22 highlights

- Linux voice/video: WebKit WebRTC enabled, portal/PipeWire env, minimal media constraints
- Recv-only video when desktop has no camera (still see Android camera)
- Android spinner deadlock fix; iroh online timeout

### v0.2.21 highlights

- Settings shows where received files are saved; open folder or copy path
- Transfer progress shows MB/s; file messages have “Open location”
- Android starts without ANR (background engine init); FileProvider for opening files
- Desktop “Test mic & camera”; libenchant spellcheck warnings suppressed in `run.sh`

### v0.2.20 highlights

- SAS confirm no longer crashes (ratchet responder must receive initiator's first message)
- Initiator sends `ratchet_open` bootstrap after trust — both peers can message

### v0.2.19 highlights

- macOS iroh DNS fix (`scutil --dns`); `SRLTCP_DNS` override for router hijack
- WebKit GStreamer video constraint fix for calls

### v0.2.18 highlights

- Chat video Play/Pause/Open controls; call ICE queuing and permission fixes

### v0.2.17 highlights

- Incoming call answer dialog; call overlay; peer presence; display names; serial I/O

### v0.2.16 highlights

- Voice/video calls via WebRTC (encrypted SDP/ICE signaling)
- File transfer ACK protocol — screenshots/images complete reliably
- Cancel in-flight transfers; images preview in chat
- Trusted peer auto-reconnect + outbound message queue

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
├── core/                         # Rust: crypto, iroh, serial, UniFFI
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
git tag -a v0.2.22 -m "SRLTCP v0.2.22"
git push origin main
git push origin v0.2.22
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