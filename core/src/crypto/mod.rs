//! Cryptographic primitives for SRLTCP v0.2.0.
//!
//! - Ed25519 long-term identity
//! - Hybrid X25519 + ML-KEM-768 key exchange
//! - AES-256-GCM session encryption (hardware-accelerated via aws-lc-rs)
//! - Double Ratchet for messaging
//! - QR + SAS discovery/verification

pub mod handshake;
pub mod identity;
pub mod ratchet;

pub use handshake::{HandshakeError, HybridKeyExchange, SessionCipher};
pub use identity::{compute_sas, parse_qr_payload, Identity, IdentityError, ParsedQr};
pub use ratchet::{DoubleRatchet, RatchetError};