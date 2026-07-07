# User Guide

Using SRLTCP v0.2.0 for secure peer-to-peer messaging.

## Getting Started

### Desktop

1. Clone the repository to your desktop
2. Open a terminal in the `SRLTCPv2` folder
3. Run `./run.sh` (Linux/macOS) or `run.bat` (Windows)
4. The app window opens — you're ready to connect

First launch takes a few minutes to compile. Subsequent launches are instant.

### Android

1. Build and install the APK (see [BUILD.md](BUILD.md))
2. Open SRLTCP — the background service starts automatically
3. A notification appears: "Listening for peers..."
4. You can safely swipe the app away or press Home

## Connecting to a Peer

### Method 1: QR Code + SAS (Recommended)

1. **Share identity:** Click "Copy QR Payload" in the sidebar. Send it to your peer (or display as QR in a future update).
2. **Connect:** Use serial or LAN/WAN connection (see below).
3. **Verify:** Paste your peer's QR payload and click "Handshake + SAS".
4. **Compare:** Both sides see a 6-digit code. Verify verbally or through a known channel.
5. **Trust:** If codes match, the connection is authenticated.

### Method 2: Serial Cable

1. Connect a serial/USB cable between two devices
2. Select the port from the dropdown (e.g., `/dev/ttyUSB0` or `COM3`)
3. Click "Connect Serial"
4. Complete SAS verification

Supported baud rates: 115200 (default), 230400, 460800, 921600.

### Method 3: LAN/WAN (QUIC)

1. Ensure both devices are on the same network (or port-forward 9473)
2. Enter the peer's IP address and port (e.g., `192.168.1.10:9473`)
3. Click "Connect LAN/WAN"
4. Complete SAS verification

## Sending Messages

1. Select a connected peer from the sidebar
2. Type your message in the input field
3. Press Enter or click Send
4. Messages support full Unicode including emojis 🔒

## File Transfer

1. Select a peer
2. Drag and drop a file or folder into the chat area (desktop)
3. Transfer progress appears as a progress bar
4. If interrupted, transfer resumes from the last acknowledged chunk

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

Both methods:
- Close serial ports
- Disconnect QUIC sessions
- Release network ports
- Clean up resources

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
| App won't start | Check `cargo` is installed; run `run.sh` to auto-install |
| No serial ports listed | Check cable connection; Linux: add user to `dialout` group |
| Can't connect via LAN | Check firewall allows UDP/TCP 9473; verify IP address |
| SAS codes don't match | Possible MITM — do not trust the connection; retry |
| Port already in use | Run `./cleanup.sh` then restart |
| Android service stopped | Disable battery optimization for SRLTCP |
| Slow serial transfer | Normal — 115200 baud ≈ 10 KB/s; use shorter cables |

## Privacy Tips

- Always verify the SAS code on first contact with a new peer
- Use serial for air-gapped or high-security environments
- The app does not store messages on disk by default
- Force Stop on Android clears all session state from memory