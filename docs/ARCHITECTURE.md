# Architecture

SRLTCP v0.3.0 system architecture.

## High-Level Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      User Interface                          │
│  ┌──────────────────┐       ┌──────────────────────────┐    │
│  │  Tauri Desktop   │       │  Android (Compose)       │    │
│  │  HTML/CSS/JS     │       │  + Foreground Service    │    │
│  │  + WebRTC media  │       │  + WebRTC media          │    │
│  └────────┬─────────┘       └──────────┬───────────────┘    │
│           │ Tauri IPC                    │ UniFFI JNI          │
│           ▼                            ▼                     │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              srltcp-core (Rust Library)              │    │
│  │  ┌─────────┐ ┌─────────┐ ┌──────────┐ ┌─────────┐  │    │
│  │  │   P2P   │ │ Crypto  │ │ Protocol │ │Transfer │  │    │
│  │  │ Engine  │ │ Module  │ │ Messages │ │ Chunked │  │    │
│  │  └────┬────┘ └─────────┘ └──────────┘ └─────────┘  │    │
│  │       │                                               │    │
│  │  ┌────┴────────────────────────────────┐             │    │
│  │  │         Transport Adapters           │             │    │
│  │  ├──────────┬──────────┬───────────────┤             │    │
│  │  │  Serial  │   iroh   │ Call signaling│             │    │
│  │  │ COBS+ACK │ NAT trav │  (E2EE wire)  │             │    │
│  │  └──────────┴──────────┴───────────────┘             │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
         │              │                │
         ▼              ▼                ▼
    ┌─────────┐   ┌──────────┐    ┌───────────┐
    │  UART   │   │ iroh P2P │    │ WebRTC    │
    │ /dev/tty│   │ relay/HP │    │ DTLS media│
    └─────────┘   └──────────┘    └───────────┘
```

## Core Components

### P2P Engine (`core/src/p2p/engine.rs`)

Central coordinator:

- Ed25519 identity (`P2pEngine::with_identity` for production; seed persisted by platforms)
- Peer session map (`peer:{pubkey}` canonical ids)
- `peer_aliases` — maps stale `iroh:{node}` ids to canonical sessions
- Outbound queue for trusted saved peers when offline
- Auto-reconnect with backoff using saved QR payloads
- iroh + serial transport routing
- Encrypted wire frames (handshake + double ratchet payloads)
- QR ticket refresh (bound to same Ed25519 identity)

### Identity persistence (v0.3.0)

| Platform | Storage |
|----------|---------|
| Desktop | `~/.local/share/srltcp/identity.seed` (hex, mode 0600); override with `SRLTCP_DATA_DIR` |
| Android | EncryptedSharedPreferences (`identity_seed_hex`) via Android Keystore master key |

### Network Transport (`core/src/network/iroh_transport.rs`)

**iroh 1.0** — primary WAN/LAN path:

- N0 relay preset + hole punching — **no port forwarding**
- ALPN `srltcp/1` bidirectional streams
- QR v4 embeds shareable `EndpointTicket`

### Serial Transport (`core/src/serial/`)

COBS frames + reliability layer for USB/UART links. Out-of-order buffer capped for DoS resistance.

### Crypto Module (`core/src/crypto/`)

| Layer | Implementation |
|-------|----------------|
| Identity | Ed25519 sign/verify, QR v4, seed export |
| Handshake | Hybrid X25519 + ML-KEM-768, Ed25519-signed wire steps |
| SAS | Canonical transcript (steps 1→2→3) |
| Messaging | double-ratchet-2 (Signal-spec) |

### Transfer Module (`core/src/transfer/`)

- 4 KB chunks, SHA-256 manifest
- Selective ACK; cancel via action message
- Unique storage names: `{transfer_id_prefix}_{filename}`

### WebRTC (`core` signaling + platform media)

- SDP/ICE relayed as encrypted call messages
- Desktop: browser `RTCPeerConnection` in webview
- Android: Stream WebRTC Android
- Media: STUN + DTLS-SRTP (not app-layer E2EE)

## Trusted Reconnect

1. Verified contacts store Ed25519 pubkey hex + QR payload locally
2. Per-peer chat history persisted alongside contacts
3. `load_trusted_pubkeys()` + `register_saved_peer()` on startup
4. Local identity seed restored so *peers* recognize us
5. UI auto-reconnects last active verified contact; SAS skipped when pubkey matches
6. Fresh handshake on reconnect; outbound queue flushes after auto-trust

## Android Background

`SrltcpForegroundService` holds the UniFFI engine (`START_STICKY`). UI polls events; P2P stays up when app is backgrounded.

## Technology Stack

| Layer | Technology |
|-------|------------|
| Core | Rust 2021, tokio |
| Desktop | Tauri v2, HTML/JS, WebRTC |
| Android | Kotlin Compose, UniFFI, Stream WebRTC |
| Crypto | aws-lc-rs, ml-kem, double-ratchet-2, zeroize |
| Networking | **iroh 1.0** |
| Serial | serialport + COBS protocol |
