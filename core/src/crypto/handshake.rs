//! Hybrid post-quantum handshake: X25519 + ML-KEM-768.

use hkdf::Hkdf;
use std::convert::TryFrom;

use ml_kem::array::Array;
use ml_kem::kem::{Decapsulate, Encapsulate, Kem, KeyExport};
use ml_kem::ml_kem_768::Ciphertext;
use ml_kem::{DecapsulationKey768, EncapsulationKey768, MlKem768};
use rand::rngs::OsRng;
use sha2::Sha256;
use thiserror::Error;
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::{Zeroize, Zeroizing};

use super::identity::compute_sas;

#[derive(Debug, Error)]
pub enum HandshakeError {
    #[error("handshake not complete")]
    NotComplete,
    #[error("invalid message: {0}")]
    InvalidMessage(String),
    #[error("crypto error: {0}")]
    Crypto(String),
}

/// Hybrid key exchange combining classical X25519 and post-quantum ML-KEM-768.
///
/// Layout (wire):
/// - Initiator msg: X25519_pk(32) || ML-KEM-768_ek(1184)
/// - Responder msg: X25519_pk(32) || ML-KEM_ct(1088)  [+ optional bob ratchet pk appended by PeerCrypto]
///
/// Shared secret = HKDF-SHA256(X25519_ss || ML-KEM_ss, info="srltcp-v2-hybrid-kex").
pub struct HybridKeyExchange {
    x25519_secret: Option<EphemeralSecret>,
    x25519_public: PublicKey,
    mlkem_dk: Option<DecapsulationKey768>,
    mlkem_ek_bytes: Vec<u8>,
    shared_secret: Option<Zeroizing<Vec<u8>>>,
}

impl HybridKeyExchange {
    pub fn initiator() -> Self {
        let x25519_secret = EphemeralSecret::random_from_rng(OsRng);
        let x25519_public = PublicKey::from(&x25519_secret);
        let (dk, ek) = MlKem768::generate_keypair();
        let ek_bytes = ek.to_bytes().to_vec();

        Self {
            x25519_secret: Some(x25519_secret),
            x25519_public,
            mlkem_dk: Some(dk),
            mlkem_ek_bytes: ek_bytes,
            shared_secret: None,
        }
    }

    pub fn responder() -> Self {
        let x25519_secret = EphemeralSecret::random_from_rng(OsRng);
        let x25519_public = PublicKey::from(&x25519_secret);

        Self {
            x25519_secret: Some(x25519_secret),
            x25519_public,
            mlkem_dk: None,
            mlkem_ek_bytes: Vec::new(),
            shared_secret: None,
        }
    }

    /// Initiator's first message: X25519 pubkey + ML-KEM encapsulation key.
    pub fn initiator_message(&self) -> Vec<u8> {
        let mut msg = Vec::with_capacity(32 + self.mlkem_ek_bytes.len());
        msg.extend_from_slice(self.x25519_public.as_bytes());
        msg.extend_from_slice(&self.mlkem_ek_bytes);
        msg
    }

    /// Responder processes initiator message and produces response.
    pub fn responder_accept(&mut self, initiator_msg: &[u8]) -> Result<Vec<u8>, HandshakeError> {
        // X25519(32) + ML-KEM-768 encapsulation key (1184)
        if initiator_msg.len() < 32 + 1184 {
            return Err(HandshakeError::InvalidMessage("too short".into()));
        }

        let x25519_remote = PublicKey::from(
            <[u8; 32]>::try_from(&initiator_msg[..32])
                .map_err(|_| HandshakeError::InvalidMessage("bad x25519 key".into()))?,
        );
        let mlkem_remote_ek = &initiator_msg[32..32 + 1184];

        let x25519_secret = self
            .x25519_secret
            .take()
            .ok_or_else(|| HandshakeError::Crypto("x25519 secret already used".into()))?;
        let x25519_shared = x25519_secret.diffie_hellman(&x25519_remote);

        let ek_key = Array::try_from(mlkem_remote_ek)
            .map_err(|_| HandshakeError::InvalidMessage("invalid ML-KEM key length".into()))?;
        let ek = EncapsulationKey768::new(&ek_key)
            .map_err(|e| HandshakeError::Crypto(format!("invalid ML-KEM key: {e}")))?;
        let (ct, mlkem_shared) = ek.encapsulate();

        let shared = Self::combine_secrets(x25519_shared.as_bytes(), mlkem_shared.as_slice());
        self.shared_secret = Some(shared);

        // Response: X25519_pk || ML-KEM ciphertext only (no unused second EK).
        let mut response = Vec::with_capacity(32 + 1088);
        response.extend_from_slice(self.x25519_public.as_bytes());
        response.extend_from_slice(ct.as_ref());
        Ok(response)
    }

    /// Initiator processes responder message.
    ///
    /// Accepts both:
    /// - New format: 32 + 1088 (+ optional 32-byte ratchet pk handled by PeerCrypto strip)
    /// - Legacy format: 32 + 1088 + 1184 unused EK (+ optional ratchet pk)
    pub fn initiator_finish(&mut self, responder_msg: &[u8]) -> Result<(), HandshakeError> {
        let ct_size = 1088;
        let min_len = 32 + ct_size;
        if responder_msg.len() < min_len {
            return Err(HandshakeError::InvalidMessage("too short".into()));
        }

        let x25519_remote = PublicKey::from(
            <[u8; 32]>::try_from(&responder_msg[..32])
                .map_err(|_| HandshakeError::InvalidMessage("bad x25519 key".into()))?,
        );
        let mlkem_ct = &responder_msg[32..32 + ct_size];

        let x25519_secret = self
            .x25519_secret
            .take()
            .ok_or_else(|| HandshakeError::Crypto("x25519 secret already used".into()))?;
        let x25519_shared = x25519_secret.diffie_hellman(&x25519_remote);

        let dk = self
            .mlkem_dk
            .take()
            .ok_or_else(|| HandshakeError::Crypto("no ML-KEM secret".into()))?;
        let ct = Ciphertext::try_from(mlkem_ct)
            .map_err(|_| HandshakeError::InvalidMessage("invalid ML-KEM ciphertext length".into()))?;
        let mlkem_shared = dk.decapsulate(&ct);

        let combined = Self::combine_secrets(x25519_shared.as_bytes(), mlkem_shared.as_slice());
        self.shared_secret = Some(combined);
        Ok(())
    }

    fn combine_secrets(x25519: &[u8], mlkem: &[u8]) -> Zeroizing<Vec<u8>> {
        let mut input = Zeroizing::new(Vec::with_capacity(x25519.len() + mlkem.len()));
        input.extend_from_slice(x25519);
        input.extend_from_slice(mlkem);
        let hk = Hkdf::<Sha256>::new(None, &input);
        let mut okm = vec![0u8; 32];
        hk.expand(b"srltcp-v2-hybrid-kex", &mut okm)
            .expect("HKDF expand");
        Zeroizing::new(okm)
    }

    pub fn shared_secret(&self) -> Option<&[u8]> {
        self.shared_secret.as_deref().map(|s| s.as_slice())
    }

    pub fn take_shared_secret(&mut self) -> Option<Zeroizing<Vec<u8>>> {
        self.shared_secret.take()
    }

    pub fn sas(&self, local_pk: &[u8], remote_pk: &[u8]) -> Option<String> {
        self.shared_secret
            .as_ref()
            .map(|s| compute_sas(s, local_pk, remote_pk))
    }
}

impl Drop for HybridKeyExchange {
    fn drop(&mut self) {
        self.mlkem_ek_bytes.zeroize();
        // shared_secret is Zeroizing; x25519/mlkem keys drop with crate impls
    }
}
