//! Cryptographic primitives for SRLTCP.
//!
//! - Ed25519 long-term identity (persistable seed)
//! - Hybrid X25519 + ML-KEM-768 key exchange
//! - Signal-spec Double Ratchet messaging (double-ratchet-2)
//! - QR + SAS discovery/verification
//! - Secrets zeroized on drop where possible

pub mod handshake;
pub mod identity;
pub mod peer_crypto;
pub mod ratchet;
pub mod wire;

pub use handshake::{HandshakeError, HybridKeyExchange};
pub use identity::{
    compute_sas, compute_sas_with_transcript, load_or_create_seed_file, parse_qr_payload, write_seed_file,
    Identity, IdentityError, IdentitySeed, ParsedQr,
};
pub use peer_crypto::{PeerCrypto, TrustState};
pub use ratchet::{RatchetEnvelope, RatchetError, SessionRatchet};
pub use wire::{EncryptedPayload, HandshakeTranscript, SignedHandshake, WireFrame};