//! Network transports: QUIC (LAN/WAN) and relay fallback.

pub mod quic;

pub use quic::{QuicError, QuicTransport};

/// Transport type selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransportKind {
    Serial,
    Lan,
    Wan,
    Relay,
}