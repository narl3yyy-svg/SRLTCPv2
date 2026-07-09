# Architecture

SRLTCP v0.2.0 system architecture.

## High-Level Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      User Interface                          │
│  ┌──────────────────┐       ┌──────────────────────────┐    │
│  │  Tauri Desktop   │       │  Android (Compose)       │    │
│  │  HTML/CSS/JS     │       │  + Foreground Service    │    │
│  └────────┬─────────┘       └──────────┬───────────────┘    │
│           │                            │                     │
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
│  │  │  Serial  │   iroh   │    WebRTC     │             │    │
│  │  │ COBS+ACK │ NAT trav │  (signaling)  │             │    │
│  │  └──────────┴──────────┴───────────────┘             │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
         │              │                │
         ▼              ▼                ▼
    ┌─────────┐   ┌──────────┐    ┌───────────┐
    │  UART   │   │ iroh P2P │    │  Media    │
    │ /dev/tty│   │ relay/HP │    │  Streams  │
    └─────────┘   └──────────┘    └───────────┘
```

## Core Components

### P2P Engine (`core/src/p2p/engine.rs`)

Central coordinator that:

- Manages the local Ed25519 identity
- Starts/stops transport adapters
- Tracks peer sessions and connection state
- Routes messages to the correct transport
- Emits events to UI layers (Tauri events / UniFFI callbacks)
- Handles graceful shutdown of all resources

### Serial Transport (`core/src/serial/`)

Three-layer stack:

1. **Frame** — COBS encoding + CRC32
2. **Reliability** — Sequence numbers, ACK/NACK, retransmit
3. **Transport** — Async serial port I/O with event channel

### Network Transport (`core/src/network/`)

iroh 1.0 for NAT traversal:

- N0 relay preset + hole punching (no port forwarding)
- ALPN `srltcp/1` application streams
- QR v4 embeds shareable `EndpointTicket`
- Connection registry per peer; graceful close on shutdown

### Crypto Module (`core/src/crypto/`)

- **Identity** — Ed25519 keygen, sign, verify, QR encoding
- **Handshake** — Hybrid X25519 + ML-KEM-768 with SAS
- **Ratchet** — double-ratchet-2 (Signal-spec) for ongoing messages

### Transfer Module (`core/src/transfer/`)

Chunked file/folder transfer:

- 4KB chunks (fits serial frames)
- SHA-256 manifest for integrity
- Selective ACK for partial retransmit
- Resumable after disconnect

### WebRTC Module (`core/src/webrtc/`)

Voice/video calling:

- SDP offer/answer over encrypted P2P channel
- ICE candidate exchange via ChatMessage types
- E2EE signaling; DTLS-SRTP for media

## Data Flow: Sending a Message

```
User types message
       │
       ▼
ChatMessage::text() ─── JSON serialize
       │
       ▼
SessionRatchet::encrypt() ─── double-ratchet-2
       │
       ▼
Envelope::new(encrypted=true) ─── JSON serialize
       │
       ├─── Serial path ──────────────────────┐
       │    ReliabilityLayer::prepare_send()   │
       │    Frame::data() → COBS + CRC        │
       │    UART write                         │
       │                                       │
       └─── iroh path ────────────────────────┤
            bidirectional stream write         │
                                               ▼
                                         Peer receives
                                               │
                                               ▼
                                    Reverse pipeline → UI
```

## Android Background Architecture

```
┌──────────────────────────────────────┐
│ MainActivity (Compose UI)            │
│  - Chat interface                    │
│  - QR display / scan                 │
│  - Starts service on onCreate()      │
│  - Does NOT stop service on destroy  │
└──────────────┬───────────────────────┘
               │ startForegroundService()
               ▼
┌──────────────────────────────────────┐
│ SrltcpForegroundService              │
│  - Persistent notification           │
│  - START_STICKY (restarts if killed) │
│  - stopWithTask=false                │
│  - Holds UniFFI SrltcpEngine ref     │
│  - P2P listen + active sessions      │
└──────────────────────────────────────┘
```

## Graceful Shutdown Sequence

```
Signal (Ctrl+C / window close / ACTION_STOP)
       │
       ▼
1. P2pEngine::shutdown()
       │
       ├── SerialTransport::shutdown()
       │     ├── Send FIN frame
       │     ├── Flush write buffer
       │     └── Close port handle
       │
       ├── QuicTransport::shutdown()
       │     ├── Close all connections
       │     └── Wait idle + close endpoint
       │
       ├── Clear peer sessions
       └── Drop crypto state (keys zeroed)
       │
       ▼
2. Remove PID file, release ports
```

## Technology Stack

| Layer | Technology |
|-------|------------|
| Core language | Rust 2021 |
| Async runtime | tokio |
| Desktop shell | Tauri v2 |
| Desktop UI | HTML/CSS/JS (Svelte-ready) |
| Android UI | Kotlin + Jetpack Compose |
| Android bindings | UniFFI-rs |
| Crypto backend | aws-lc-rs |
| Post-quantum | ml-kem 0.3 (ML-KEM-768) |
| Networking | quinn (QUIC) |
| Serial | serialport + custom protocol |
| Logging | tracing + tracing-subscriber |