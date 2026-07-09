//! Signal-spec Double Ratchet via double-ratchet-2.

use double_ratchet_2::header::Header;
use double_ratchet_2::ratchet::Ratchet;
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};

pub const RATCHET_PK_LEN: usize = 32;

#[derive(Debug, Error)]
pub enum RatchetError {
    #[error("decryption failed")]
    DecryptFailed,
    #[error("invalid state")]
    InvalidState,
    #[error("encode error: {0}")]
    Encode(String),
}

/// Serialized ratchet ciphertext on the wire (v0.2.13+).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RatchetEnvelope {
    pub header_bytes: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 12],
}

/// Application-layer Double Ratchet session.
pub struct SessionRatchet {
    inner: Ratchet<StaticSecret>,
    is_initiator: bool,
    /// Bob's ratchet DH public key (init_bob output) — sent in handshake step 2.
    bob_ratchet_pk: Option<PublicKey>,
}

impl SessionRatchet {
    /// Responder path: init_bob; returns ratchet DH pubkey to embed in handshake step 2.
    pub fn init_responder(shared_secret: &[u8]) -> Result<(Self, PublicKey), RatchetError> {
        let sk = derive_ratchet_sk(shared_secret);
        let (inner, bob_pk) = Ratchet::<StaticSecret>::init_bob(sk);
        Ok((
            Self {
                inner,
                is_initiator: false,
                bob_ratchet_pk: Some(bob_pk),
            },
            bob_pk,
        ))
    }

    /// Initiator path: init_alice with bob's ratchet DH pubkey from step 2.
    pub fn init_initiator(shared_secret: &[u8], bob_ratchet_pk: &PublicKey) -> Self {
        let sk = derive_ratchet_sk(shared_secret);
        let inner = Ratchet::<StaticSecret>::init_alice(sk, *bob_ratchet_pk);
        Self {
            inner,
            is_initiator: true,
            bob_ratchet_pk: None,
        }
    }

    pub fn bob_ratchet_pk(&self) -> Option<&PublicKey> {
        self.bob_ratchet_pk.as_ref()
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<RatchetEnvelope, RatchetError> {
        let ad = b"srltcp-v3";
        let (header, ciphertext, nonce) = self.inner.ratchet_encrypt(plaintext, ad);
        Ok(RatchetEnvelope {
            header_bytes: header.concat(ad),
            ciphertext,
            nonce,
        })
    }

    pub fn decrypt(&mut self, envelope: &RatchetEnvelope) -> Result<Vec<u8>, RatchetError> {
        let ad = b"srltcp-v3";
        let header = Header::<PublicKey>::from(envelope.header_bytes.as_slice());
        let plaintext = self.inner.ratchet_decrypt(
            &header,
            &envelope.ciphertext,
            &envelope.nonce,
            ad,
        );
        Ok(plaintext)
    }

    /// Legacy bytes wrapper for wire migration: postcard-encoded envelope.
    pub fn encrypt_to_bytes(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, RatchetError> {
        let env = self.encrypt(plaintext)?;
        postcard::to_allocvec(&env).map_err(|e| RatchetError::Encode(e.to_string()))
    }

    pub fn decrypt_from_bytes(&mut self, data: &[u8]) -> Result<Vec<u8>, RatchetError> {
        let env: RatchetEnvelope = postcard::from_bytes(data)
            .map_err(|_| RatchetError::DecryptFailed)?;
        self.decrypt(&env)
    }
}

fn derive_ratchet_sk(shared_secret: &[u8]) -> [u8; 32] {
    let mut okm = [0u8; 32];
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, shared_secret);
    hk.expand(b"srltcp-v3-ratchet-root", &mut okm)
        .expect("HKDF expand");
    okm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_style_roundtrip() {
        let secret = [9u8; 32];
        let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
        let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);

        let pt = b"test message for ratchet";
        let env = alice.encrypt(pt).unwrap();
        let dec = bob.decrypt(&env).unwrap();
        assert_eq!(dec, pt);

        let (env2, _, _) = bob.inner.ratchet_encrypt(b"reply", b"srltcp-v3");
        let header = Header::<PublicKey>::from(env2.concat(b"srltcp-v3").as_slice());
        // bob can encrypt after receiving first message
        let _ = env2;
        let env_a = alice.encrypt(b"second").unwrap();
        let _ = bob.decrypt(&env_a).unwrap();
        let _ = header;
    }
}