# Changelog

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

### Known limitations (future work)

- Folder transfer UI wiring (zstd backend ready, end-to-end flow in progress).
- WebRTC voice/video E2EE signaling not yet ratchet-wrapped.
- Voice notes UI not yet exposed (Audio message type exists).
- QUIC transport still uses ephemeral TLS certs; identity binding is at application handshake layer.

## v0.2.8

- SAS modal visibility, Android keyboard/IME send, saved contacts, serial device labels, settings panels.

## v0.2.7

- QR double-port fix, `run.sh --pull`, Android copy QR.