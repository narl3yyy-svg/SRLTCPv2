//! Hybrid post-quantum handshake: X25519 + ML-KEM-768.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
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
pub struct HybridKeyExchange {
    x25519_secret: Option<EphemeralSecret>,
    x25519_public: PublicKey,
    mlkem_dk: Option<DecapsulationKey768>,
    mlkem_ek_bytes: Vec<u8>,
    shared_secret: Option<Vec<u8>>,
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
        if initiator_msg.len() < 32 + 1184 {
            return Err(HandshakeError::InvalidMessage("too short".into()));
        }

        let x25519_remote = PublicKey::from(<[u8; 32]>::try_from(&initiator_msg[..32]).unwrap());
        let mlkem_remote_ek = &initiator_msg[32..];

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

        let (dk, ek_local) = MlKem768::generate_keypair();
        self.mlkem_dk = Some(dk);
        let ek_local_bytes = ek_local.to_bytes();

        let shared = Self::combine_secrets(x25519_shared.as_bytes(), mlkem_shared.as_slice());
        self.shared_secret = Some(shared);

        let mut response = Vec::new();
        response.extend_from_slice(self.x25519_public.as_bytes());
        response.extend_from_slice(ct.as_ref());
        response.extend_from_slice(ek_local_bytes.as_ref());
        Ok(response)
    }

    /// Initiator processes responder message.
    pub fn initiator_finish(&mut self, responder_msg: &[u8]) -> Result<(), HandshakeError> {
        let ct_size = 1088;
        let ek_size = 1184;
        if responder_msg.len() < 32 + ct_size + ek_size {
            return Err(HandshakeError::InvalidMessage("too short".into()));
        }

        let x25519_remote = PublicKey::from(<[u8; 32]>::try_from(&responder_msg[..32]).unwrap());
        let mlkem_ct = &responder_msg[32..32 + ct_size];
        let mlkem_remote_ek = &responder_msg[32 + ct_size..];

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
        let mlkem_shared1 = dk.decapsulate(&ct);

        let ek_key = Array::try_from(mlkem_remote_ek)
            .map_err(|_| HandshakeError::InvalidMessage("invalid ML-KEM key length".into()))?;
        let ek = EncapsulationKey768::new(&ek_key)
            .map_err(|e| HandshakeError::Crypto(format!("invalid ML-KEM key: {e}")))?;
        let (_, mlkem_shared2) = ek.encapsulate();

        let mut mlkem_combined = Vec::with_capacity(64);
        mlkem_combined.extend_from_slice(mlkem_shared1.as_slice());
        mlkem_combined.extend_from_slice(mlkem_shared2.as_slice());
        let combined = Self::combine_secrets(x25519_shared.as_bytes(), &mlkem_combined);
        self.shared_secret = Some(combined);
        Ok(())
    }

    fn combine_secrets(x25519: &[u8], mlkem: &[u8]) -> Vec<u8> {
        let mut input = Vec::with_capacity(x25519.len() + mlkem.len());
        input.extend_from_slice(x25519);
        input.extend_from_slice(mlkem);
        let hk = Hkdf::<Sha256>::new(None, &input);
        let mut okm = vec![0u8; 32];
        hk.expand(b"srltcp-v2-hybrid-kex", &mut okm)
            .expect("HKDF expand");
        okm
    }

    pub fn shared_secret(&self) -> Option<&[u8]> {
        self.shared_secret.as_deref()
    }

    pub fn sas(&self, local_pk: &[u8], remote_pk: &[u8]) -> Option<String> {
        self.shared_secret
            .as_ref()
            .map(|s| compute_sas(s, local_pk, remote_pk))
    }
}

/// Session cipher derived from handshake — AES-256-GCM.
pub struct SessionCipher {
    cipher: Aes256Gcm,
    send_nonce: u64,
    recv_nonce: u64,
}

impl SessionCipher {
    pub fn from_shared_secret(secret: &[u8], salt: &[u8]) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(salt), secret);
        let mut key = [0u8; 32];
        hk.expand(b"srltcp-v2-session", &mut key).expect("HKDF");
        Self {
            cipher: Aes256Gcm::new_from_slice(&key).expect("key init"),
            send_nonce: 0,
            recv_nonce: 0,
        }
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, HandshakeError> {
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[4..].copy_from_slice(&self.send_nonce.to_be_bytes());
        self.send_nonce += 1;

        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| HandshakeError::Crypto(e.to_string()))
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, HandshakeError> {
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[4..].copy_from_slice(&self.recv_nonce.to_be_bytes());
        self.recv_nonce += 1;

        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| HandshakeError::Crypto(e.to_string()))
    }
}