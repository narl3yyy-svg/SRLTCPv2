# Changelog

## v0.2.14 — QR paste/connect fix, iroh stability (2026-07-09)

### Fixes (critical)

- **Peer ID mismatch**: `ensure_connected` no longer returns a truncated ticket prefix — uses actual `iroh:{node_id}` so handshake succeeds after dial.
- **QR paste**: Whitespace/newlines stripped; URL-safe and standard base64 accepted.
- **iroh before QR**: Engine waits for iroh bind before generating v4 QR (desktop + Android).
- **Error surfacing**: `ConnectResult.error` field; UI shows actionable messages (expired QR, legacy v3, dial timeout).

### Tests

- QR v4 parsing suite (`qr_v4_parsing.rs`) + identity unit tests for paste normalization.

### Docs

- ML-KEM audit status and OpenMLS migration draft in `docs/CRYPTO.md`.

## v0.2.13 — iroh NAT traversal, Signal Double Ratchet (2026-07-08)

### Transport

- **iroh 1.0** replaces quinn/QUIC — relay + hole punching, no port forwarding required.
- QR **v4** embeds iroh `EndpointTicket` alongside Ed25519 identity.
- WAN endpoint settings removed (desktop + Android); NAT handled by iroh.

### Crypto

- **double-ratchet-2** — Signal-spec Double Ratchet for 1:1 messaging (replaces simplified ratchet).
- Hybrid **X25519 + ML-KEM-768** unchanged; handshake step 2 appends bob ratchet DH pubkey (32 bytes).
- Encrypted payload version bumped to `3` (postcard `RatchetEnvelope`).

### Tests

- Wire dump test: no application plaintext on wire.
- Signal KAT suite mirroring double-ratchet-2 upstream tests.
- `ratchet_bench` criterion benchmark.

### API

- `iroh_ticket()` replaces `local_endpoint()` (deprecated alias kept).
- `connect_quic()` now dials iroh tickets.

## v0.2.12 — Peer UX, postcard wire format, reconnect model (2026-07-08)

### Fixes

- **Peer UX**: Disconnect vs Remove — disconnect ends transport only; saved contact, trust, and chat history are kept. Remove revokes trust and deletes history.
- **Duplicate peers**: `reconcilePeers()` dedupes transient `quic:` entries after canonical `peer:{pubkey}` IDs.
- **Trusted reconnect**: Saved verified peers reconnect without re-SAS when Ed25519 identity matches stored trust.
- **Post-verify messaging**: Connection/trust guards on send path; handshake timeout cleans up stale state.
- **Android QR UX**: Your QR hidden while chatting; connect sheet clears peer QR field; clear button on QR input.
- **Chat history**: Per-peer persistence on Android and desktop; survives soft disconnect.
- **File send**: Requires live connection + trusted session; clearer errors when offline or unverified.
- **Voice/video**: Buttons disabled with honest "WebRTC coming soon" message (no fake call flow).

### Wire format

- **Postcard binary frames** (`SR` magic + postcard) replace JSON for `WireFrame` on the wire. Legacy JSON frames still deserialize for backward compatibility.

### Known limitations (unchanged)

- Simplified Double Ratchet (not full Signal-style ratchet).
- Handshake replay hardening and QUIC streaming reads still planned.
- WebRTC media path not integrated.

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