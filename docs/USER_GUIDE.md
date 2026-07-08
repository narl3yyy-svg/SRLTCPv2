# User Guide

Using SRLTCP v0.2.6 for secure peer-to-peer messaging.

## Getting Started

### Desktop

1. Clone the repository to your desktop
2. Open a terminal in the `SRLTCPv2` folder
3. Run `./run.sh` (Linux/macOS) or `run.bat` (Windows)
4. The app window opens — no compilation required

The launcher downloads a prebuilt binary from GitHub Releases. Use `./run.sh --rebuild` only if you are developing from source.

### Android

1. Download `SRLTCPv2-0.2.6.apk` from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases/latest) or build locally (see [BUILD.md](BUILD.md))
2. Open SRLTCP — the background service starts automatically
3. A notification appears: "Listening for peers..."
4. You can safely swipe the app away or press Home

## Connecting to a Peer

### QR Code + SAS (Required)

SRLTCP v0.2.6 uses QR-based discovery only — there is no manual IP connect option. QR codes (v3) embed the peer's LAN address so connection starts automatically.

1. **Share identity:** Copy or display your QR code. Send the payload to your peer.
2. **Paste peer QR:** Open **Add Peer**, paste their QR payload, and click **Connect & Verify (QR + SAS)**.
3. **Compare:** Both sides see a 6-digit SAS code. Verify verbally or through a known channel.
4. **Trust:** If codes match, click **Codes Match — Trust Peer**. Messaging unlocks.
5. **Mismatch:** If codes differ, disconnect immediately — possible man-in-the-middle attack.

### Serial Cable (Desktop, Optional)

1. Connect a serial/USB cable between two machines
2. Open **Add Peer** → **Serial cable (optional)**
3. Select the port (e.g., `/dev/ttyUSB0` or `COM3`) and click **Connect Serial**
4. Complete SAS verification with QR exchange

Supported baud rates: 115200 (default), 230400, 460800, 921600.

## Sending Messages

1. Select a connected, **verified** peer from the sidebar
2. Type your message in the input field
3. Press Enter or click Send
4. Messages support full Unicode including emojis

## File Transfer

1. Select a verified peer
2. Click the file button and choose a file (desktop)
3. Transfer progress appears as a progress bar
4. Images and videos display inline in chat when supported

## Voice and Video Calls

1. Select a verified peer
2. Click the phone or camera icon
3. WebRTC establishes the media connection
4. Signaling is E2EE over the P2P channel

## Android Background Use

SRLTCP on Android is designed to stay connected:

- **Swipe away the app** — Service keeps running, notification stays
- **Press Home** — Same behavior
- **Receive messages** — P2P core processes them in background
- **Stop completely** — Settings → Apps → SRLTCP → Force Stop

## Shutting Down

### Desktop

- Press **Ctrl+C** in the terminal running `run.sh`
- Or close the app window (triggers graceful shutdown)

Both methods close serial ports, disconnect QUIC sessions, release network ports, and clean up resources.

### Full Cleanup

If something goes wrong or you want a completely clean state:

```bash
./cleanup.sh        # Linux/macOS
cleanup.bat         # Windows
```

This kills all SRLTCP processes and releases port 9473.

### Android

Settings → Apps → SRLTCP → Force Stop

## Troubleshooting

| Problem | Solution |
|---------|----------|
| `run.sh` says no prebuilt | Install from [Releases](https://github.com/narl3yyy-svg/SRLTCPv2/releases) or use `--rebuild` |
| No peers appear | Share your QR; ensure both devices allow UDP/TCP 9473 through firewall |
| No serial ports listed | Check cable connection; Linux: add user to `dialout` group |
| SAS codes don't match | Possible MITM — do not trust the connection; retry |
| Port already in use | Run `./cleanup.sh` then restart |
| Android service stopped | Disable battery optimization for SRLTCP |
| Slow serial transfer | Normal — 115200 baud ≈ 10 KB/s; use shorter cables |

## Privacy Tips

- Always verify the SAS code on first contact with a new peer
- Use serial for air-gapped or high-security environments
- The app does not store messages on disk by default
- Force Stop on Android clears all session state from memory