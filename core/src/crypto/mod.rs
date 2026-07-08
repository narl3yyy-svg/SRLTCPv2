//! Cryptographic primitives for SRLTCP v0.2.0.
//!
//! - Ed25519 long-term identity
//! - Hybrid X25519 + ML-KEM-768 key exchange
//! - AES-256-GCM session encryption (hardware-accelerated via aws-lc-rs)
//! - Double Ratchet for messaging
//! - QR + SAS discovery/verification

pub mod handshake;
pub mod identity;
pub mod peer_crypto;
pub mod ratchet;
pub mod wire;

pub use handshake::{HandshakeError, HybridKeyExchange, SessionCipher};
pub use identity::{
    compute_sas, compute_sas_with_transcript, parse_qr_payload, Identity, IdentityError, ParsedQr,
};
pub use peer_crypto::{PeerCrypto, TrustState};
pub use ratchet::{DoubleRatchet, RatchetError};
pub use wire::{EncryptedPayload, HandshakeTranscript, SignedHandshake, WireFrame};