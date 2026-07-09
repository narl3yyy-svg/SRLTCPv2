//! Network transports: iroh (NAT traversal) + optional serial.

pub mod iroh_transport;
pub mod local;

pub use iroh_transport::{IrohError, IrohTransport, SRLTCP_ALPN};
pub use local::detect_lan_ip;

/// Transport type selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransportKind {
    Serial,
    Lan,
    Wan,
    Relay,
}