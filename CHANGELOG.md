# Changelog

## v0.3.2 — Linux mic/camera WebKit permission (2026-07-11)

### Critical

- **Linux WebKitGTK getUserMedia** — auto-allow `permission-request` so mic/camera work without a browser-style popup (fixes silent `NotAllowedError` on Ubuntu/Arch).
- **run.sh** — PipeWire/portal env hardened; quieter default `RUST_LOG`.

### Notes

- WebKit does **not** show a system permission dialog; the app grants media access when you click **Grant / test mic & camera** or start a call.
- Requires PipeWire + wireplumber + xdg-desktop-portal (already typical on modern desktops).


## v0.3.2 — Android call UX, notifications, prebuilt launcher (2026-07-11)

### Critical fixes

- **Android release crash on open** — JNA R8 keep rules + iroh `ndk_context` init (shipped on main after v0.3.0).
- **Android MasterKeys / WebRTC lambda / R8** — release APK builds and launches.
- **GitHub Release includes Android APK** — slim arm64-v8a `SRLTCPv2-0.3.2.apk`.
- **`./run.sh` prebuilt download** — robust platform tag (`linux-x86_64` not distro ID), HTML rejection, fallback to latest GitHub Release when workspace version is ahead of CI.

### Calls & disconnect

- **Call ends both sides** when one peer hangs up or the transport drops.
- **Android call UI** — always-visible End / Mute / Speaker bar (not covered by video or system nav).
- **Disconnect peer** hangs up any active call first, then tears down the session (works even if the remote side still shows connected briefly).
- **Speakerphone toggle** on Android during calls; improved voice-call audio mode.

### Notifications & audio

- **Android** — request `POST_NOTIFICATIONS` on first launch; high-priority message/call channels; full-screen intent for incoming calls.
- **Desktop** — optional system notifications for messages; Settings toggle + permission button.
- **Desktop audio devices** — choose microphone and speaker/output (where WebView supports `setSinkId`).

### Other

- Quieter desktop logs: suppress iroh QAD multi-path WARN spam (`RUST_LOG` default).
- Version 0.3.2 across Cargo, Android, Tauri, docs.

## v0.3.0 — Production hardening (2026-07-11)

### Security

- **Persistent identity** — Ed25519 seed survives restarts (desktop seed file mode 0600; Android EncryptedSharedPreferences / Keystore).
- **Zeroization** — Hybrid KEX shared secrets and identity seeds use `zeroize` / `Zeroizing`.
- **QR refresh binding** — Ticket updates accepted only when Ed25519 pubkey matches the session peer (blocks identity swap).
- **Serial DoS** — Out-of-order receive buffer hard-capped.
- **Hybrid step-2 cleanup** — Removed unused responder ML-KEM EK from new handshakes; legacy bodies still parse.
- **Honest WebRTC docs** — Media is DTLS-SRTP, not Double-Ratchet E2EE (UI privacy note).
- Removed unused `aes-gcm` / `hashbrown` direct deps; `zeroize` actually used.

### Lightweight

- Workspace **release profile**: LTO, `codegen-units=1`, strip, `opt-level=s`, `panic=abort`.
- QR `image` crate: **PNG-only** features (no TIFF/EXR/AVIF bloat).
- Android default **arm64-v8a only** release APK + ProGuard/R8 + resource shrink; `SRLTCP_UNIVERSAL_APK=1` for multi-ABI.
- `build-android.sh` uses `assembleRelease` and strips native `.so` files.

### UX

- Desktop first-run welcome banner, fingerprint display, light/dark via `prefers-color-scheme`.
- Settings privacy notes for calls and local chat storage.
- Call lifecycle / QR refresh improvements from 0.2.32 line absorbed.

### Docs & repo

- LICENSE-MIT + LICENSE-APACHE + CONTRIBUTING.md added.
- README, SECURITY, CRYPTO, ARCHITECTURE, BUILD, USER_GUIDE updated for 0.3.0.
- Identity persistence tests added.
- Prebuilt binaries removed from git tracking (GitHub Releases only); `dist/.gitkeep` retained.

### Residual risks (unchanged class)

- `double-ratchet-2` still pre-release; chat history not encrypted at rest; no third-party audit.

## v0.2.31 — Linux launcher & prebuilt fix (2026-07-11)

### Fixes

- **`./run.sh --rebuild` broken** — Cargo build log was captured as the binary path (stdout pollution via `tee`). Build output now goes to stderr; `--rebuild` stages via `build-desktop.sh` and launches `dist/bin/linux-x86_64/srltcp-desktop`.
- **Local fallback build** — Auto-build after failed GitHub download uses `find_staged_binary()` instead of version-gated `find_binary()`.
- **Clearer errors** — "Binary missing or invalid" now shows the path tried and suggests `build-desktop.sh`.
- **CI / release** — `srltcp-desktop-linux-x86_64` published on GitHub Releases (v0.2.30+); `build-desktop.sh` logs to stderr so scripts stay pipe-safe.

## v0.2.30 — Transfers, calls, reconnect, Android UX (2026-07-11)

### Fixes

- **File transfer direction** — Sender no longer shows received-side bubbles on upload complete (desktop + Android). Sender preview kept at pick time; receiver gets image/video preview when download completes.
- **Transfer reliability** — File chunks use the same encrypted send path as chat (`send_wire_message`); stale iroh sessions torn down before redial; dead connections unregistered on loss.
- **Saved contact reconnect** — Reconnect prep no longer marks peers user-paused or emits false disconnect events; fresh iroh dial; desktop re-registers saved peer + trust on success.
- **Voice/video calls** — Remote audio unmuted on desktop; intentional hangup flag prevents UI races; Android handles `call_end`, enables communication audio mode, aspect-fit remote video PiP layout.
- **Call/video layout** — Desktop overlay uses 16:9 aspect ratio with `object-fit: contain`; chat image previews sized proportionally.
- **Android back gesture** — Back closes dialogs/sheets (including SAS) instead of leaving verify screen; moves app to background when nothing is open.

## v0.2.29 — Contacts, persistence, startup reconnect (2026-07-10)

### Fixes

- **Contact persistence** — Saved contacts and per-peer chat history survive app restart (desktop `localStorage`; Android SharedPreferences). Legacy storage keys migrated on desktop.
- **SAS auto-save** — Verified peers and QR payload saved automatically on SAS confirm or auto-trusted reconnect — no manual save step.
- **Delete contact clears UI** — Remove wipes contact, chat history, transfers, and closes the chat pane on desktop and Android.
- **Short display names** — Peer lists show remote display name when known, otherwise first 12 hex chars of pubkey (not full `peer:…` id).
- **Startup reconnect without re-SAS** — On launch, trusted saved contacts register with the engine; last active (or most recent) verified peer reconnects automatically, skipping SAS when pubkey matches stored trust.
- **Android crash/ANR fixes** — UI renders before engine init; native work on IO; cold-start reconnect does not block the main thread.

### Documentation

- **Honest security docs** — `docs/SECURITY.md` lists residual risks: unaudited crypto, plaintext-at-rest chat/contacts, WebRTC media path, no OS keystore.

### Polish (post-release)

- **Desktop delete contact** — Removing a contact no longer switches to another peer's chat; the chat pane closes cleanly.
- **Desktop version labels** — `index.html` title and sidebar show v0.2.29.
- **Desktop saved-contact chips** — Chip row lists saved contacts (offline included), matching Android.
- **Desktop auto-trusted event** — `sas_ready` with `autoTrusted` now persists the contact and QR like manual SAS confirm.
- **Legacy storage migration** — Desktop loads contacts from v0.2.26–v0.2.28 storage keys.

## v0.2.27 — Android version label fix (2026-07-10)

### Fixes

- **Status bar showed v0.2.22** — Top bar and Settings now read `BuildConfig.VERSION_NAME` instead of a hardcoded string.

## v0.2.26 — Android startup ANR fix (2026-07-10)

### Fixes

- **Android freeze / ANR on launch** — Show UI immediately; never update Compose state from IO thread; decode QR bitmap off main thread; notification permission moved to Settings.

## v0.2.25 — Call stability & Android notifications (2026-07-10)

### Fixes

- **Linux video call UI freeze** — Detect `/dev/video*` via `has_local_camera`; skip `getUserMedia({ video: true })` on headless desktops; camera off by default; recv-only transceivers only.
- **Android notifications** — High-priority message/call alert channels; `POST_NOTIFICATIONS` runtime prompt; foreground service posts alerts when app is backgrounded.

### Notes

- OpenMLS evaluation deferred — current `double-ratchet-2` stack unchanged; see `docs/SECURITY.md`.

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