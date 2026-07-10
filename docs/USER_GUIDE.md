# User Guide

Using SRLTCP v0.2.29 for secure peer-to-peer messaging.

## Getting Started

### Desktop

1. Clone the repository to your desktop
2. Open a terminal in the `SRLTCPv2` folder
3. Run `./run.sh` (Linux/macOS) or `run.bat` (Windows)
4. The app window opens — no compilation required

The launcher downloads a prebuilt binary from GitHub Releases. Use `./run.sh --rebuild` only if you are developing from source.

### Android

1. Download `SRLTCPv2-0.2.29.apk` from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases/latest) or build locally (see [BUILD.md](BUILD.md))
2. Open SRLTCP — the background service starts automatically
3. A notification appears: "Listening for peers..."
4. You can safely swipe the app away or press Home

## Connecting to a Peer

### QR Code + SAS (Required)

SRLTCP v0.2.29 uses **QR v4** with an **iroh ticket** for NAT traversal — no port forwarding or WAN settings required. Paste the peer's QR and tap **Connect & Verify**.

1. **Share identity:** Copy or display your QR code. Send the payload to your peer.
2. **Paste peer QR:** Open **Add Peer**, paste their QR payload, and click **Connect & Verify (QR + SAS)**.
3. **Compare:** Both sides see a 6-digit SAS code. Verify verbally or through a known channel.
4. **Trust:** If codes match, click **Codes Match — Trust Peer**. Messaging unlocks.
5. **Mismatch:** If codes differ, disconnect immediately — possible man-in-the-middle attack.

**Important:** The peer who **started the connection** (pasted QR) should confirm SAS first. The other peer can send messages once they receive the first encrypted message (usually within a second).

### Serial Cable (Desktop, Optional)

1. Connect a serial/USB cable between two machines
2. Open **Add Peer** → **Serial cable (optional)**
3. Select the port (e.g., `/dev/ttyUSB0` or `COM3`) and click **Connect Serial**
4. Complete SAS verification with QR exchange

Supported baud rates: 115200 (default), 230400, 460800, 921600.

Click **↻ Refresh** in the serial section if your device was plugged in after the app started. If no ports appear, check the cable and (on Linux) add your user to the `dialout` group.

## Peers, Contacts, and Presence

The **Peers** panel has two sections:

| Section | What it shows |
|---------|----------------|
| **Peers Online** | Peers with an active connection right now |
| **Saved Contacts** | Verified peers saved for reconnect (stay listed when offline) |

**Presence labels:**

- **● online** — connected and reachable
- **↻ reconnecting** — connection dropped; app is retrying automatically
- **⏸ disconnected by you** — you ended the session; use **Reconnect** to chat again
- **○ offline** — saved contact not connected (peer may still be online elsewhere)

**Disconnect** ends the session but keeps the contact. The chat window closes. Use **Reconnect** on a saved contact to connect again — you will not be auto-reconnected after a manual disconnect.

**Startup auto-reconnect:** On launch, verified **Saved Contacts** register with the engine. The app reconnects to your last active peer (or most recently seen verified contact) automatically. When the stored Ed25519 pubkey matches, SAS is skipped — messaging resumes after handshake.

**Remove contact:** Deletes the saved contact, clears per-peer chat history, and closes the chat UI. Use this when you no longer want a peer listed.

**Display name:** Set yours in **Settings → Display name**. After verification, both peers see each other's names in the chat header and contact lists. Without a display name, peers show as the first 12 characters of their pubkey.

## Sending Messages

1. Open a **verified** peer from **Peers Online** or reconnect a **Saved Contact**
2. Type your message in the input field
3. Press Enter or click Send
4. Messages support full Unicode including emojis

## File Transfer

1. Select a verified peer
2. Click the file button and choose a file (desktop)
3. Transfer progress appears as a progress bar with **MB/s** throughput
4. Images and videos display inline in chat. Videos include **Play/Pause** controls (and **Open** on desktop); tap **Cancel** to abort an in-flight transfer
5. Received files save to the folder shown in **Settings → Received files** (desktop) or **Settings → Files save to** (Android). Tap **Open location** / **Open file** on a file message to reveal the save folder.
6. Messages to offline saved contacts queue automatically and send on reconnect

## Voice and Video Calls

1. Select a verified, **online** peer
2. Click **Voice** or **Video** in the chat toolbar (desktop) or the call icons (Android)
3. **Incoming calls:** An answer dialog appears — tap **Answer** or **Decline**. You must answer before mic/camera access is granted (required on Linux/WebKit).
4. During a call: use **End**, **Mute**, and **Camera** controls in the call overlay (desktop) or the call bar (Android)
5. **Call settings** (desktop): Settings → enable/disable microphone and camera; use **Test mic & camera** to verify portal permissions (Linux). Allow the xdg-desktop-portal prompt when it appears.
6. **Call permissions** (Android): Settings → **Grant mic & camera permissions** before placing calls
6. Signaling is E2EE over the P2P channel; media uses WebRTC (STUN/DTLS-SRTP)

**Linux tip:** If voice/video fails with a permission error, allow PipeWire/portal mic and camera access for the app in system settings, then retry after clicking **Answer**.

## Android Background Use

SRLTCP on Android is designed to stay connected:

- A foreground service keeps the P2P engine alive
- Battery optimization should be disabled for SRLTCP (Settings → Apps → SRLTCP → Battery → Unrestricted)
- Swiping the app away does not stop the service

## Shutting Down

### Desktop

```bash
./cleanup.sh        # Linux/macOS
cleanup.bat         # Windows
```

This kills all SRLTCP processes and releases resources.

### Android

Settings → Apps → SRLTCP → Force Stop

## Troubleshooting

| Problem | Solution |
|---------|----------|
| `run.sh` says no prebuilt | Install from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases) or use `--rebuild` |
| Android app stuck on loading | Update to **v0.2.29+** (UI shows before engine init; work runs on IO thread) |
| SAS confirm does nothing / crash | Both peers on **v0.2.29+**; initiator (who pasted QR) confirms first |
| No peers in Peers Online | Only connected peers appear there; check **Saved Contacts** and tap **Reconnect** |
| Peer shows offline but is up | They may have disconnected from you; reconnect from Saved Contacts |
| macOS relay/DNS errors in terminal | `export SRLTCP_DNS=10.0.50.1` (router IP from `scutil --dns`) then `./run.sh` |
| GStreamer GstIntRange warnings (Linux) | Suppressed in `run.sh`; harmless if seen |
| Voice call permission denied | Linux: Settings → **Test mic & camera**, allow portal prompt; retry call |
| Video call, no local camera | Desktop receives remote video (recv-only); Android camera still works |
| Video won't play | Use the Play button or native controls; on desktop try **Open** to play in your system player |
| Transfer stuck | Wait for ACK progress; cancel and retry; both peers on v0.2.29+ |
| No serial ports listed | Plug in device, click **Refresh**; Linux: add user to `dialout` group |
| Serial connect fails | Both peers on v0.2.29+; try another baud rate; check cable |
| SAS codes don't match | Possible MITM — do not trust the connection; retry |
| Port already in use | Run `./cleanup.sh` then restart |
| Android service stopped | Disable battery optimization for SRLTCP |
| Slow serial transfer | Normal — 115200 baud ≈ 10 KB/s; use shorter cables |

## Privacy Tips

- Always verify the SAS code on first contact with a new peer
- Use serial for air-gapped or high-security environments
- **Chat history is persisted per peer** locally (desktop `localStorage`; Android app storage) — survives restart and soft disconnect. Not encrypted at rest beyond OS filesystem permissions.
- Saved contacts and QR payloads are stored locally for auto-reconnect
- Force Stop on Android clears in-memory session state; persisted contacts/chat remain on disk until you remove them or clear app data