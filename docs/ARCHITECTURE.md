# Architecture

SRLTCP v0.2.31 system architecture.

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
    │ /dev/tty│   │ relay/HP │    │ STUN media│
    └─────────┘   └──────────┘    └───────────┘
```

## Core Components

### P2P Engine (`core/src/p2p/engine.rs`)

Central coordinator:

- Ed25519 identity and peer session map (`peer:{pubkey}` canonical ids)
- `peer_aliases` — maps stale `iroh:{node}` ids to canonical sessions
- Outbound queue for trusted saved peers when offline
- Auto-reconnect with backoff using saved QR payloads
- iroh + serial transport routing
- Encrypted wire frames (handshake + double ratchet payloads)

### Network Transport (`core/src/network/iroh_transport.rs`)

**iroh 1.0** — primary WAN/LAN path:

- N0 relay preset + hole punching — **no port forwarding**
- ALPN `srltcp/1` bidirectional streams
- QR v4 embeds shareable `EndpointTicket`
- Connection registry per peer; rekey on handshake canonicalization

Legacy QUIC/quinn and port 9473 forwarding were removed in v0.2.13.

### Serial Transport (`core/src/serial/`)

COBS frames + reliability layer for USB/UART links.

### Crypto Module (`core/src/crypto/`)

| Layer | Implementation |
|-------|----------------|
| Identity | Ed25519 sign/verify, QR v4 |
| Handshake | Hybrid X25519 + ML-KEM-768, Ed25519-signed wire steps |
| SAS | Canonical transcript (steps 1→2→3) |
| Messaging | double-ratchet-2 (Signal-spec, Curve25519/X25519 ecosystem) |

**Note:** `ml-kem` 0.3 is Wycheproof-tested but not independently audited. `double-ratchet-2` is pre-release (0.4.0-pre.2).

### Transfer Module (`core/src/transfer/`)

- 4 KB chunks, SHA-256 manifest
- Selective ACK wired on receive — sender completes when all chunks ACKed
- Cancel via `action: "cancel"` message
- Unique storage names: `{transfer_id_prefix}_{filename}`

### WebRTC (`core` signaling + platform media)

- SDP offer/answer/ICE relayed as encrypted `CallOffer` / `CallAnswer` / `CallIce` messages
- Desktop: browser `RTCPeerConnection` in webview
- Android: Stream WebRTC Android
- Media uses STUN; signaling is E2EE over iroh

## Data Flow: Encrypted Message

```
User input → ChatMessage JSON → PeerCrypto::encrypt (double-ratchet-2)
    → WireFrame::Encrypted → iroh bi-stream → peer
    → resolve_session_peer → decrypt → UI event
```

## Trusted Reconnect

1. Verified contacts store Ed25519 pubkey hex + QR payload locally (desktop `localStorage`; Android SharedPreferences)
2. Per-peer chat history persisted alongside contacts
3. `load_trusted_pubkeys()` + `register_saved_peer()` on startup
4. UI auto-reconnects last active (or most recent) verified contact without re-SAS when pubkey matches
5. Fresh handshake on reconnect; SAS skipped on auto-trusted path
6. Outbound queue flushes after auto-trust
7. Engine auto-reconnects with exponential backoff on connection loss

## Android Background

`SrltcpForegroundService` holds the UniFFI engine (`START_STICKY`). UI polls events; P2P stays up when app is backgrounded.

## Technology Stack

| Layer | Technology |
|-------|------------|
| Core | Rust 2021, tokio |
| Desktop | Tauri v2, HTML/JS, WebRTC |
| Android | Kotlin Compose, UniFFI, Stream WebRTC |
| Crypto | aws-lc-rs, ml-kem, double-ratchet-2 |
| Networking | **iroh 1.0** (not quinn) |
| Serial | serialport + COBS protocol |