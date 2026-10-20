//! iroh transport — NAT traversal via relay + hole punching (no port forwarding).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use iroh::dns::{DnsProtocol, DnsResolver};
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, endpoint::presets};
use iroh_tickets::Ticket;
use iroh_tickets::endpoint::EndpointTicket;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// ALPN for SRLTCP application protocol over iroh.
pub const SRLTCP_ALPN: &[u8] = b"srltcp/1";

/// Largest single wire frame accepted on one stream.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum IrohError {
    #[error("endpoint error: {0}")]
    Endpoint(String),
    #[error("connection error: {0}")]
    Connection(String),
    #[error("not running")]
    NotRunning,
    #[error("peer not connected: {0}")]
    PeerNotFound(String),
    #[error("ticket error: {0}")]
    Ticket(String),
}

/// A registered connection and which side dialed it.
struct ConnEntry {
    conn: Connection,
    outbound: bool,
}

/// Outcome of [`IrohTransport::register`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterOutcome {
    /// Connection is now the current one for this peer id.
    Registered,
    /// Connection replaced a live connection from the other direction (dial glare);
    /// anything sent on the old one may have been lost.
    ReplacedLive,
    /// A live connection from the other direction won the glare tie-break; the new
    /// connection was closed and the existing one stays current.
    Rejected,
}

/// iroh endpoint wrapper with connection registry.
pub struct IrohTransport {
    endpoint: Option<Endpoint>,
    connections: Arc<RwLock<HashMap<String, ConnEntry>>>,
    online: bool,
}

impl Default for IrohTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl IrohTransport {
    pub fn new() -> Self {
        Self {
            endpoint: None,
            connections: Arc::new(RwLock::new(HashMap::new())),
            online: false,
        }
    }

    /// Bind iroh endpoint with N0 relay preset. Returns the endpoint so the caller can wait
    /// for [`Self::wait_online`] without holding a lock on the transport.
    pub async fn bind(&mut self) -> Result<Endpoint, IrohError> {
        let dns = build_dns_resolver();
        let ep = Endpoint::builder(presets::N0)
            .alpns(vec![SRLTCP_ALPN.to_vec()])
            .dns_resolver(dns)
            .bind()
            .await
            .map_err(|e| IrohError::Endpoint(e.to_string()))?;
        self.endpoint = Some(ep.clone());
        Ok(ep)
    }

    /// Wait (bounded) until the endpoint has a relay / direct addresses for NAT traversal.
    pub async fn wait_online(ep: &Endpoint) {
        match tokio::time::timeout(Duration::from_secs(45), ep.online()).await {
            Ok(()) => info!("iroh endpoint online (NAT traversal active)"),
            Err(_) => warn!("iroh online() timed out after 45s — relay may connect later"),
        }
    }

    pub fn mark_online(&mut self) {
        self.online = true;
    }

    pub fn is_bound(&self) -> bool {
        self.endpoint.is_some()
    }

    /// Bound and past the initial online wait — tickets now carry relay/direct addresses.
    pub fn is_online(&self) -> bool {
        self.endpoint.is_some() && self.online
    }

    pub fn endpoint_addr(&self) -> Result<EndpointAddr, IrohError> {
        let ep = self.endpoint.as_ref().ok_or(IrohError::NotRunning)?;
        Ok(ep.addr())
    }

    /// Shareable ticket string for QR / out-of-band discovery.
    pub fn ticket_string(&self) -> Result<String, IrohError> {
        let addr = self.endpoint_addr()?;
        Ok(EndpointTicket::new(addr).to_string())
    }

    pub fn parse_ticket(ticket: &str) -> Result<EndpointAddr, IrohError> {
        let t = EndpointTicket::decode_string(ticket.trim())
            .map_err(|e| IrohError::Ticket(e.to_string()))?;
        Ok(t.endpoint_addr().clone())
    }

    /// Dial remote peer by iroh endpoint address (from ticket).
    pub async fn connect(&self, addr: EndpointAddr) -> Result<Connection, IrohError> {
        let ep = self.endpoint.as_ref().ok_or(IrohError::NotRunning)?;
        Self::dial(ep, addr).await
    }

    /// Dial using an endpoint handle, so callers need not hold the transport lock while the
    /// relay / hole-punch handshake runs.
    pub async fn dial(ep: &Endpoint, addr: EndpointAddr) -> Result<Connection, IrohError> {
        let remote = addr.id;
        let conn = tokio::time::timeout(Duration::from_secs(45), ep.connect(addr, SRLTCP_ALPN))
            .await
            .map_err(|_| {
                IrohError::Connection(
                    "timed out after 45s (relay/hole-punch may be slow — retry with fresh QR)"
                        .into(),
                )
            })?
            .map_err(|e| IrohError::Connection(e.to_string()))?;
        info!(%remote, "iroh outbound connection established");
        Ok(conn)
    }

    /// Endpoint handle for the accept loop (cheap clone; lets the loop wait for incoming
    /// connections without holding the transport lock).
    pub fn endpoint(&self) -> Option<Endpoint> {
        self.endpoint.clone()
    }

    /// Register a connection for `peer_id`.
    ///
    /// When both peers dial each other at the same time each ends up with an inbound and an
    /// outbound connection. Both sides keep the connection dialed by the endpoint with the
    /// smaller id, so they converge on the same one without any extra round trip.
    pub async fn register(&self, peer_id: String, conn: Connection, outbound: bool) -> RegisterOutcome {
        let local_id = self.endpoint.as_ref().map(|ep| ep.id());
        let mut map = self.connections.write().await;
        let mut outcome = RegisterOutcome::Registered;
        if let Some(existing) = map.get(&peer_id) {
            if existing.conn.stable_id() == conn.stable_id() {
                return RegisterOutcome::Registered;
            }
            let existing_live = existing.conn.close_reason().is_none();
            if existing_live && existing.outbound != outbound {
                if let Some(local) = local_id {
                    let remote = conn.remote_id();
                    let dialer = |out: bool| if out { *local.as_bytes() } else { *remote.as_bytes() };
                    if dialer(existing.outbound) < dialer(outbound) {
                        conn.close(0u32.into(), b"glare");
                        return RegisterOutcome::Rejected;
                    }
                }
                outcome = RegisterOutcome::ReplacedLive;
            }
            existing.conn.close(0u32.into(), b"replaced");
        }
        map.insert(peer_id, ConnEntry { conn, outbound });
        outcome
    }

    pub async fn unregister(&self, peer_id: &str) {
        if let Some(entry) = self.connections.write().await.remove(peer_id) {
            entry.conn.close(0u32.into(), b"disconnect");
        }
    }

    /// Unregister only if `stable_id` is still the current connection for `peer_id`.
    /// Returns false when the connection was already replaced (e.g. by a reconnect).
    pub async fn unregister_if_current(&self, peer_id: &str, stable_id: usize) -> bool {
        let mut map = self.connections.write().await;
        if map.get(peer_id).is_some_and(|e| e.conn.stable_id() == stable_id) {
            if let Some(entry) = map.remove(peer_id) {
                entry.conn.close(0u32.into(), b"disconnect");
            }
            return true;
        }
        false
    }

    /// Stable id of the current connection for `peer_id`.
    pub async fn connection_id(&self, peer_id: &str) -> Option<usize> {
        self.connections
            .read()
            .await
            .get(peer_id)
            .map(|e| e.conn.stable_id())
    }

    /// Move a connection to a new key (transient `iroh:` id → canonical `peer:` id).
    /// A different connection already under `new_id` is an older session and is closed.
    pub async fn rekey(&self, old_id: &str, new_id: &str) {
        let mut map = self.connections.write().await;
        if let Some(entry) = map.remove(old_id) {
            let moved_id = entry.conn.stable_id();
            if let Some(prev) = map.insert(new_id.to_string(), entry) {
                if prev.conn.stable_id() != moved_id {
                    prev.conn.close(0u32.into(), b"replaced");
                }
            }
        }
    }

    /// True when a live (not closed) connection is registered for `peer_id`.
    pub async fn has_connection(&self, peer_id: &str) -> bool {
        self.connections
            .read()
            .await
            .get(peer_id)
            .is_some_and(|e| e.conn.close_reason().is_none())
    }

    /// Send framed bytes over a new bidirectional stream.
    pub async fn send(&self, peer_id: &str, data: &[u8]) -> Result<(), IrohError> {
        let conn = {
            let map = self.connections.read().await;
            map.get(peer_id)
                .map(|e| e.conn.clone())
                .ok_or_else(|| IrohError::PeerNotFound(peer_id.to_string()))?
        };

        let (mut send, recv) = conn
            .open_bi()
            .await
            .map_err(|e| IrohError::Connection(e.to_string()))?;

        if data.len() > MAX_FRAME_BYTES {
            return Err(IrohError::Connection(format!(
                "frame too large ({} bytes, max {MAX_FRAME_BYTES})",
                data.len()
            )));
        }
        send.write_all(data)
            .await
            .map_err(|e| IrohError::Connection(e.to_string()))?;
        send.finish()
            .map_err(|e| IrohError::Connection(e.to_string()))?;
        drop(recv);

        Ok(())
    }

    pub async fn shutdown(&mut self) {
        let mut conns = self.connections.write().await;
        for (_, entry) in conns.drain() {
            entry.conn.close(0u32.into(), b"shutdown");
        }

        if let Some(ep) = self.endpoint.take() {
            ep.close().await;
            info!("iroh endpoint shut down");
        }
        self.online = false;
    }
}

/// DNS for iroh relay hostnames. iroh's default macOS reader often fails on some networks;
/// routers that hijack DNS (reply from 10.x instead of 8.8.8.8) break the Google fallback.
fn build_dns_resolver() -> DnsResolver {
    if let Ok(raw) = std::env::var("SRLTCP_DNS") {
        let servers = parse_dns_list(&raw);
        if !servers.is_empty() {
            info!(dns = ?servers, "iroh DNS from SRLTCP_DNS");
            return dns_from_ips(&servers);
        }
    }

    let mut servers = Vec::new();
    #[cfg(target_os = "macos")]
    {
        servers.extend(macos_scutil_nameservers());
    }
    servers.extend(resolv_conf_nameservers());

    if servers.is_empty() {
        warn!(
            "iroh DNS: no system servers found — using 1.1.1.1 and 8.8.8.8. \
             On macOS with router DNS hijacking, set: export SRLTCP_DNS=10.0.50.1"
        );
        servers.extend([
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
        ]);
    } else {
        info!(dns = ?servers, "iroh DNS from system config");
    }

    dns_from_ips(&servers)
}

fn parse_dns_list(raw: &str) -> Vec<IpAddr> {
    raw.split([',', ' '])
        .filter_map(|part| part.trim().parse::<IpAddr>().ok())
        .collect()
}

fn dns_from_ips(servers: &[IpAddr]) -> DnsResolver {
    let mut builder = DnsResolver::builder();
    for ip in servers {
        builder = builder.with_nameserver(SocketAddr::new(*ip, 53), DnsProtocol::Udp);
    }
    builder.build()
}

fn resolv_conf_nameservers() -> Vec<IpAddr> {
    let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        let Some(ip_str) = line.strip_prefix("nameserver") else {
            continue;
        };
        let ip_str = ip_str.trim();
        if let Ok(ip) = ip_str.parse::<IpAddr>() {
            if !out.contains(&ip) {
                out.push(ip);
            }
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn macos_scutil_nameservers() -> Vec<IpAddr> {
    use std::process::Command;
    let output = match Command::new("scutil").arg("--dns").output() {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.contains("nameserver[") {
            continue;
        }
        let Some(ip_str) = line.split(':').nth(1).map(str::trim) else {
            continue;
        };
        if let Ok(ip) = ip_str.parse::<IpAddr>() {
            if !out.contains(&ip) {
                out.push(ip);
            }
        }
    }
    out
}

/// Read loop helper: accept bi streams and deliver bytes to callback.
pub async fn read_connection_loop<F, Fut>(conn: Connection, peer_id: String, mut on_data: F)
where
    F: FnMut(String, Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    loop {
        match conn.accept_bi().await {
            Ok((_send, mut recv)) => {
                match recv.read_to_end(MAX_FRAME_BYTES).await {
                    Ok(data) if !data.is_empty() => {
                        on_data(peer_id.clone(), data).await;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, peer = %peer_id, "iroh stream read error");
                        break;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, peer = %peer_id, "iroh accept_bi closed");
                break;
            }
        }
    }
}