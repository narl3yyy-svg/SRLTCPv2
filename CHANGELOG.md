# Changelog

## v0.2.11 — Peer routing, file transfer, trusted reconnect (2026-07-08)

### Fixes

- **Canonical peer IDs** (`peer:{pubkey}`) — fixes file send, calls, and video failing due to `quic:` socket mismatch between inbound/outbound connections.
- **File chunk transfer** — encrypted chunk messages now sent/received over the wire (was offer-only before).
- **Trusted peer reconnect** — saved verified contacts auto-reconnect and skip SAS dialog when Ed25519 identity matches (new handshake still runs for fresh session keys).
- **Incoming call offers** — receiver now gets `CallStarted` event.
- **Android upload** — clearer error when file copy from URI fails; receive dir set to `files/received`.

### API

- `ConnectResult.auto_trusted`, `load_trusted_pubkeys()`, `set_receive_dir()`, `PeerIdUpdated` event.

## v0.2.10 — SAS fix + WAN connect (2026-07-08)

### Security

- **SAS mismatch fixed**: Canonical handshake transcript (step bodies 1→2→3, length-prefixed) now recorded symmetrically on initiator and responder — both peers derive the same 6-digit SAS.
- **Engine handshake wiring**: Initiator records step 1 before sending; uses `initiator_process_step2` + `initiator_finalize_step3`; responder uses `responder_process_step1` + `responder_process_step3`.
- Unit test `sas_matches_both_sides_with_canonical_transcript` validates matching SAS.

### Features

- **WAN endpoint**: Optional `host:port` in Settings (desktop + Android). Connect & Verify tries LAN from QR first, then falls back to configured WAN.
- `set_wan_endpoint()` / `wan_endpoint()` exposed via UniFFI and Tauri.

### Cleanup

- Removed unused `core/examples/` dev binaries, unused `SessionCipher`, and `snow` dependency.

### Known limitations

- QUIC transport uses ephemeral TLS certs; identity binding is at application handshake layer.
- Folder transfer UI, WebRTC E2EE signaling, and voice notes UI still in progress.

## v0.2.9 — Security-first crypto overhaul (2026-07-08)

### Security (critical fixes)

- **Wire handshake**: Hybrid X25519 + ML-KEM-768 KEX now runs over QUIC/serial with Ed25519-signed frames — no more local simulation.
- **E2EE messaging**: All application messages encrypted with Double Ratchet (AES-256-GCM via aws-lc-rs) before transmission.
- **SAS binding**: 6-digit SAS derived from shared secret + long-term identities + full handshake transcript.
- **Explicit trust**: Peers enter `SasPending` after handshake; messaging requires `confirm_peer_trusted()` after user verifies SAS.
- **Identity verification**: Handshake frames must match QR Ed25519 public key.

### Performance

- zstd compression module added for folder/bulk transfer (`core/src/transfer/compress.rs`).
- Double Ratchet chain advance fixed (every 256 messages).
- Hardware-accelerated AES-GCM through aws-lc-rs / aes-gcm.

### UX

- **Desktop QR image**: CSP updated to allow `data:` URLs — visual QR renders correctly.
- **Android QR image**: Visual QR code displayed via `qr_image_data_url()` from Rust core.
- Prebuilt-first `run.sh` (no auto-compile on `git pull`).

### Architecture

- Per-peer `PeerCrypto` state machine: `Connected → Handshaking → SasPending → Trusted`.
- `WireFrame` protocol: signed handshake steps + encrypted payloads.

## v0.2.8

- SAS modal visibility, Android keyboard/IME send, saved contacts, serial device labels, settings panels.

## v0.2.7

- QR double-port fix, `run.sh --pull`, Android copy QR.