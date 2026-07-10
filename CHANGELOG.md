# Changelog

## v0.2.24 — Desktop startup fix (2026-07-10)

### Fixes (critical)

- **Desktop stuck on "Connecting…"** — JavaScript syntax error in `app.js` (`??` mixed with `||`) prevented the UI from loading; engine was online but status never updated.

## v0.2.23 — Calls, transfers, contacts UI (2026-07-10)

### Fixes (critical)

- **Recv-only calls** — Desktop and Android support listen/watch-only when no mic/camera (Arch headless ↔ Android video works).
- **Transfer cleanup on disconnect** — Core cancels in-flight transfers; no infinite chunk-retry spam; UI clears progress bars.
- **Android transfer MB/s** — Incoming transfers use `total_size` from engine progress events.
- **Image preview** — Transfer complete uses actual storage path; chat shows inline images on both platforms.
- **Reconnect → chat** — Auto-trusted reconnect opens chat (not stuck on QR); desktop switches to Peers panel.
- **Android contacts** — Full-screen dialog instead of bottom sheet; contact chips shown when contacts exist.

### Notes

- Screen/window share planned for a future release (requires `getDisplayMedia` + signaling work).

## v0.2.22 — Linux voice/video calls & Android spinner fix (2026-07-10)

### Fixes (critical)

- **Linux voice/video calls** — Enable WebKit WebRTC/media-stream in Tauri setup; portal + PipeWire env in `run.sh`; minimal `getUserMedia` constraints (no `enumerateDevices`, no audio constraint objects that trigger GstIntRange); recv-only video when local camera unavailable.
- **Android infinite spinner / ANR** — Engine returns immediately; iroh binds in background (fixes mutex deadlock from `start()` + `waitUntilReady()`). `getOrCreate()` no longer blocks the main thread; Peers sheet and SAS confirm run engine work on IO.
- **iroh online hang** — 45s timeout on `ep.online()`; overall start timeout in `wait_until_ready`.

### Linux notes

- First call: Settings → **Test mic & camera**, allow the portal prompt.
- GstIntRange GStreamer logs suppressed via `GST_DEBUG=*:0` in `run.sh`.
- Video without local camera: receive-only mode (Android/desktop camera still works one-way).

## v0.2.21 — Save path, transfer speed, Android launch fix (2026-07-10)

### Features

- **Received files location** — Settings shows the save folder path; copy path or open folder (desktop + Android).
- **Open file location** — Received file messages include an “Open location” / “Open file” action in chat.
- **Transfer MB/s** — Progress bars show throughput during send/receive (desktop + Android).

### Fixes

- **Android launch hang** — Engine init moved off the main thread (`startInBackground` / `awaitEngine`); loading spinner until ready.
- **Android file open** — `FileProvider` for opening received files on Android 7+.
- **Mic/camera permissions** — Desktop “Test mic & camera” button; Android settings grant button; simplified WebRTC video constraints (no GstIntRange spam).
- **libenchant warnings** — `ENCHANT_MODULE_DIR=/dev/null` in `run.sh` suppresses harmless WebKit spellcheck plugin noise.

## v0.2.20 — SAS confirm ratchet panic fix (2026-07-10)

### Fixes (critical)

- **SAS confirm / add peer crash** — Responder role could not encrypt before receiving the initiator's first ratchet message (`cks.unwrap()` panic in double-ratchet-2). Now returns a safe error, initiator sends a `ratchet_open` bootstrap after trust, and responder can reply after decrypting it.

## v0.2.19 — macOS DNS & WebKit video fixes (2026-07-10)

### Fixes

- **macOS iroh connectivity** — Parse `scutil --dns` / `/etc/resolv.conf` for relay DNS instead of broken system reader + Google fallback (fixes router DNS hijack on networks like `10.0.50.1`). Override with `SRLTCP_DNS=10.0.50.1`.
- **Linux GStreamer GstIntRange** — Simpler WebRTC video constraints (no ideal+max ranges that WebKit rejects).
- **macOS launcher** — Skip Linux-only `fuser` port cleanup that printed errors on exit.

## v0.2.18 — Video playback & call reliability (2026-07-09)

### Fixes

- **Video playback (desktop)** — Chat videos show native controls plus Play/Pause/Open toolbar; CSP allows `asset:` URLs for Tauri file playback.
- **Video playback (Android)** — `MediaController` on received videos; explicit Play/Pause buttons in chat.
- **Voice/video calls (desktop)** — ICE candidate queue before remote SDP; `video.play()` on streams; voice-only fallback when camera unavailable; auto `end_call` on connection failure; expanded CSP for WebRTC.
- **Voice/video calls (Android)** — Runtime mic/camera permission on answer; ICE queue; camera fallback to voice-only; WebRTC on IO thread with main-thread UI updates; remote video track binding fixes.

## v0.2.17 — Calls UI, presence, serial, docs (2026-07-09)

### Fixes

- **Voice/video calls (desktop)** — Incoming answer dialog (user gesture for mic/camera); call overlay with end/mute/camera; relaxed video constraints; CSP `media-src` for WebKit.
- **Disconnect behavior** — User disconnect no longer auto-reconnects; disconnect closes chat; reconnect only on `connection lost`.
- **Display names** — Profile exchange over wire; shown in chat header and contact lists after connect.
- **Peer presence** — Online / reconnecting / paused / offline status in Peers Online vs Saved Contacts.
- **Serial I/O** — Read/write loops after port open; refreshed device list UI on desktop.

### Documentation

- README tagline and BUILD.md aligned with iroh (no port 9473 forwarding).
- ARCHITECTURE, SECURITY, CRYPTO version bumps and honest audit notes.

## v0.2.16 — Calls, transfers, queue, docs (2026-07-09)

### Features

- **Voice/video calls** — WebRTC on desktop (webview) and Android (Stream WebRTC); SDP/ICE signaling over encrypted iroh channel.
- **Reliable file transfer** — selective ACK wired; screenshots and images complete; unique receive filenames.
- **Cancel transfers** — UI cancel button; `cancel_transfer` API.
- **Image preview** — received images/videos shown inline in chat (desktop + Android).
- **Offline queue** — messages/files to trusted saved peers queue until auto-reconnect.
- **Seamless reconnect** — verified contacts skip SAS; engine auto-reconnect with backoff.

### Documentation

- README, ARCHITECTURE, USER_GUIDE updated for iroh (removed port-forwarding/quinn references).
- SECURITY/CRYPTO: honest ml-kem audit status and double-ratchet-2 notes.

## v0.2.15 — Messaging fix, security cleanup (2026-07-09)

### Fixes (critical)

- **Chat send/receive broken after connect**: iroh read loops kept stale `iroh:{node}` ids after handshake canonicalized sessions to `peer:{pubkey}`. Incoming encrypted frames could not find the peer session. Fixed with `peer_aliases` map resolved on every inbound frame.
- **UI connection state**: Desktop/Android now track `connectedPeer` through `peer_id_updated` events.
- **Send errors surfaced**: Android/desktop receive `error` events when `send_message` fails (trust, offline, etc.).

### Security / cleanup

- Removed `SRLTCP_AUTO_TEST` desktop backdoor and `peer_connect` example binary.
- Removed dead WAN endpoint code from engine.

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