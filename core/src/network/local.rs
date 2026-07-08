//! Local network address detection for QR endpoint embedding.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};

/// Best-effort LAN IP via UDP route trick (no packets sent on most systems).
pub fn detect_lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() {
        return None;
    }
    Some(ip)
}

/// Human-readable `host:port` for QR payloads.
pub fn local_endpoint(port: u16) -> Option<String> {
    let ip = detect_lan_ip().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    Some(format!("{ip}:{port}"))
}