//! iroh transport — NAT traversal via relay + hole punching (no port forwarding).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, endpoint::presets};
use iroh_tickets::Ticket;
use iroh_tickets::endpoint::EndpointTicket;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tracing::info;

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
        let ep = Endpoint::builder(presets::N0)
            .alpns(vec![SRLTCP_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| IrohError::Endpoint(e.to_string()))?;

        ep.online().await;
        info!("iroh endpoint online (NAT traversal active)");
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
        let conn = ep
            .connect(addr, SRLTCP_ALPN)
            .await
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