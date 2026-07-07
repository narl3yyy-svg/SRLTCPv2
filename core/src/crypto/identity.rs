//! Long-term identity keys (Ed25519) and QR-based discovery.

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("invalid key material: {0}")]
    InvalidKey(String),
    #[error("signature verification failed")]
    BadSignature,
}

/// Ed25519 identity for long-term authentication.
#[derive(Clone)]
pub struct Identity {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl Identity {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(seed);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key_bytes())
    }

    pub fn from_public_key(bytes: &[u8]) -> Result<Self, IdentityError> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| IdentityError::InvalidKey("expected 32 bytes".into()))?;
        let verifying_key = VerifyingKey::from_bytes(&arr)
            .map_err(|e| IdentityError::InvalidKey(e.to_string()))?;
        // Verification-only identity (no private key)
        let signing_key = SigningKey::from_bytes(&[0u8; 32]);
        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        self.signing_key.sign(message).to_bytes()
    }

    pub fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), IdentityError> {
        let pk: [u8; 32] = public_key
            .try_into()
            .map_err(|_| IdentityError::InvalidKey("expected 32 bytes".into()))?;
        let sig: [u8; 64] = signature
            .try_into()
            .map_err(|_| IdentityError::InvalidKey("expected 64 bytes".into()))?;
        let vk = VerifyingKey::from_bytes(&pk)
            .map_err(|e| IdentityError::InvalidKey(e.to_string()))?;
        use ed25519_dalek::Verifier;
        vk.verify(message, &ed25519_dalek::Signature::from_bytes(&sig))
            .map_err(|_| IdentityError::BadSignature)
    }

    /// QR payload: base64-encoded public key with version prefix.
    pub fn qr_payload(&self) -> String {
        let mut payload = Vec::with_capacity(33);
        payload.push(0x02); // version
        payload.extend_from_slice(&self.public_key_bytes());
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &payload)
    }

    /// Parse QR payload into public key bytes.
    pub fn from_qr_payload(payload: &str) -> Result<[u8; 32], IdentityError> {
        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            payload,
        )
        .map_err(|e| IdentityError::InvalidKey(e.to_string()))?;
        if decoded.len() != 33 || decoded[0] != 0x02 {
            return Err(IdentityError::InvalidKey("invalid QR format".into()));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&decoded[1..]);
        Ok(key)
    }
}

/// Short Authentication String for out-of-band verification.
pub fn compute_sas(shared_secret: &[u8], local_pk: &[u8], remote_pk: &[u8]) -> String {
    let mut hasher = Sha256::new();
    // Canonical ordering prevents MITM reflection
    if local_pk < remote_pk {
        hasher.update(local_pk);
        hasher.update(remote_pk);
    } else {
        hasher.update(remote_pk);
        hasher.update(local_pk);
    }
    hasher.update(shared_secret);
    let hash = hasher.finalize();

    // 6-digit SAS (like Signal/ZRTP)
    let val = u32::from_be_bytes([0, hash[0], hash[1], hash[2]]) % 1_000_000;
    format!("{val:06}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let id = Identity::generate();
        let msg = b"test message";
        let sig = id.sign(msg);
        Identity::verify(&id.public_key_bytes(), msg, &sig).unwrap();
    }

    #[test]
    fn qr_roundtrip() {
        let id = Identity::generate();
        let qr = id.qr_payload();
        let pk = Identity::from_qr_payload(&qr).unwrap();
        assert_eq!(pk, id.public_key_bytes());
    }

    #[test]
    fn sas_is_deterministic() {
        let secret = b"shared";
        let a = [1u8; 32];
        let b = [2u8; 32];
        let sas1 = compute_sas(secret, &a, &b);
        let sas2 = compute_sas(secret, &b, &a);
        assert_eq!(sas1, sas2);
        assert_eq!(sas1.len(), 6);
    }
}