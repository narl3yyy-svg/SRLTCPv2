//! Network transports: QUIC (LAN/WAN) and relay fallback.

pub mod local;
pub mod quic;

pub use local::{detect_lan_ip, local_endpoint};
pub use quic::{QuicError, QuicTransport};

/// Transport type selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransportKind {
    Serial,
    Lan,
    Wan,
    Relay,
}