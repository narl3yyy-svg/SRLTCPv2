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

/// iroh endpoint wrapper with connection registry.
pub struct IrohTransport {
    endpoint: Option<Endpoint>,
    connections: Arc<RwLock<HashMap<String, Connection>>>,
    online: bool,
}

impl IrohTransport {
    pub fn new() -> Self {
        Self {
            endpoint: None,
            connections: Arc::new(RwLock::new(HashMap::new())),
            online: false,
        }
    }

    /// Bind iroh endpoint with N0 relay preset and bring online for NAT traversal.
    pub async fn bind(&mut self) -> Result<(), IrohError> {
        let dns = build_dns_resolver();
        let ep = Endpoint::builder(presets::N0)
            .alpns(vec![SRLTCP_ALPN.to_vec()])
            .dns_resolver(dns)
            .bind()
            .await
            .map_err(|e| IrohError::Endpoint(e.to_string()))?;

        match tokio::time::timeout(Duration::from_secs(45), ep.online()).await {
            Ok(()) => info!("iroh endpoint online (NAT traversal active)"),
            Err(_) => warn!("iroh online() timed out after 45s — relay may connect later"),
        }
        self.online = true;
        self.endpoint = Some(ep);
        Ok(())
    }

    pub fn is_bound(&self) -> bool {
        self.endpoint.is_some()
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

    /// Poll for incoming connection (250ms timeout).
    pub async fn try_accept(&self) -> Result<Option<(Connection, String)>, IrohError> {
        let ep = self.endpoint.as_ref().ok_or(IrohError::NotRunning)?;

        let incoming = match tokio::time::timeout(Duration::from_millis(250), ep.accept()).await {
            Ok(Some(accept)) => accept,
            Ok(None) => return Ok(None),
            Err(_) => return Ok(None),
        };

        let conn = match incoming.await {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };

        let remote = conn.remote_id();
        let peer_id = format!("iroh:{remote}");
        info!(%remote, "iroh inbound connection accepted");
        Ok(Some((conn, peer_id)))
    }

    pub async fn register(&self, peer_id: String, conn: Connection) {
        self.connections.write().await.insert(peer_id, conn);
    }

    pub async fn unregister(&self, peer_id: &str) {
        if let Some(conn) = self.connections.write().await.remove(peer_id) {
            conn.close(0u32.into(), b"disconnect");
        }
    }

    pub async fn rekey(&self, old_id: &str, new_id: &str) {
        let mut map = self.connections.write().await;
        if let Some(conn) = map.remove(old_id) {
            map.insert(new_id.to_string(), conn);
        }
    }

    pub async fn has_connection(&self, peer_id: &str) -> bool {
        self.connections.read().await.contains_key(peer_id)
    }

    /// Send framed bytes over a new bidirectional stream.
    pub async fn send(&self, peer_id: &str, data: &[u8]) -> Result<(), IrohError> {
        let conn = {
            let map = self.connections.read().await;
            map.get(peer_id)
                .cloned()
                .ok_or_else(|| IrohError::PeerNotFound(peer_id.to_string()))?
        };

        let (mut send, recv) = conn
            .open_bi()
            .await
            .map_err(|e| IrohError::Connection(e.to_string()))?;

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
        for (_, conn) in conns.drain() {
            conn.close(0u32.into(), b"shutdown");
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
                match recv.read_to_end(16 * 1024 * 1024).await {
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