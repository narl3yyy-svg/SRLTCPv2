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

    /// QR payload v2 (identity only) — legacy.
    pub fn qr_payload(&self) -> String {
        self.qr_payload_with_endpoint(None, crate::DEFAULT_QUIC_PORT)
    }

    /// QR payload v3 embeds LAN endpoint so peers can connect without manual IP entry.
    pub fn qr_payload_with_endpoint(&self, endpoint_host: Option<&str>, port: u16) -> String {
        if let Some(host) = endpoint_host.filter(|h| !h.is_empty()) {
            let host_bytes = host.as_bytes();
            let host_len = host_bytes.len().min(253) as u8;
            let mut payload = Vec::with_capacity(36 + host_bytes.len());
            payload.push(0x03);
            payload.extend_from_slice(&self.public_key_bytes());
            payload.push(host_len);
            payload.extend_from_slice(&host_bytes[..host_len as usize]);
            payload.extend_from_slice(&port.to_be_bytes());
            return base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                &payload,
            );
        }
        let mut payload = Vec::with_capacity(33);
        payload.push(0x02);
        payload.extend_from_slice(&self.public_key_bytes());
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &payload)
    }

    /// Parse QR payload into public key bytes.
    pub fn from_qr_payload(payload: &str) -> Result<[u8; 32], IdentityError> {
        Ok(parse_qr_payload(payload)?.public_key)
    }
}

/// Parsed peer QR: identity + optional connection endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQr {
    pub public_key: [u8; 32],
    pub endpoint: Option<String>,
}

/// Parse v2 (identity-only) or v3 (identity + endpoint) QR payloads.
pub fn parse_qr_payload(payload: &str) -> Result<ParsedQr, IdentityError> {
    let decoded = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        payload.trim(),
    )
    .map_err(|e| IdentityError::InvalidKey(e.to_string()))?;

    if decoded.is_empty() {
        return Err(IdentityError::InvalidKey("empty QR payload".into()));
    }

    match decoded[0] {
        0x02 => {
            if decoded.len() != 33 {
                return Err(IdentityError::InvalidKey("invalid v2 QR length".into()));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&decoded[1..]);
            Ok(ParsedQr {
                public_key: key,
                endpoint: None,
            })
        }
        0x03 => {
            if decoded.len() < 36 {
                return Err(IdentityError::InvalidKey("invalid v3 QR length".into()));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&decoded[1..33]);
            let host_len = decoded[33] as usize;
            if decoded.len() < 34 + host_len + 2 {
                return Err(IdentityError::InvalidKey("invalid v3 host field".into()));
            }
            let host = std::str::from_utf8(&decoded[34..34 + host_len])
                .map_err(|e| IdentityError::InvalidKey(e.to_string()))?;
            let port = u16::from_be_bytes([
                decoded[34 + host_len],
                decoded[34 + host_len + 1],
            ]);
            Ok(ParsedQr {
                public_key: key,
                endpoint: Some(format_qr_endpoint(host, port)),
            })
        }
        _ => Err(IdentityError::InvalidKey("unsupported QR version".into())),
    }
}

/// Build a connectable `host:port` from QR v3 fields.
/// Handles legacy v0.2.6 payloads that stored `ip:port` in the host field (double-port bug).
pub fn format_qr_endpoint(host: &str, port: u16) -> String {
    if !host.contains(':') {
        return format!("{host}:{port}");
    }

    let endpoint = host.to_string();
    let parts: Vec<&str> = endpoint.split(':').collect();
    if parts.len() >= 3 {
        let last = parts[parts.len() - 1];
        let prev = parts[parts.len() - 2];
        if last == prev && last.parse::<u16>().is_ok() {
            return parts[..parts.len() - 1].join(":");
        }
    }
    endpoint
}

/// Short Authentication String for out-of-band verification.
pub fn compute_sas(shared_secret: &[u8], local_pk: &[u8], remote_pk: &[u8]) -> String {
    compute_sas_with_transcript(shared_secret, local_pk, remote_pk, &[])
}

/// SAS bound to handshake transcript + long-term identities (MITM-resistant).
pub fn compute_sas_with_transcript(
    shared_secret: &[u8],
    local_pk: &[u8],
    remote_pk: &[u8],
    transcript: &[u8],
) -> String {
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
    hasher.update(transcript);
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
        let parsed = parse_qr_payload(&qr).unwrap();
        assert_eq!(parsed.public_key, id.public_key_bytes());
        assert!(parsed.endpoint.is_none());
    }

    #[test]
    fn qr_v3_roundtrip() {
        let id = Identity::generate();
        let qr = id.qr_payload_with_endpoint(Some("192.168.1.42"), 9473);
        let parsed = parse_qr_payload(&qr).unwrap();
        assert_eq!(parsed.public_key, id.public_key_bytes());
        assert_eq!(parsed.endpoint.as_deref(), Some("192.168.1.42:9473"));
    }

    #[test]
    fn qr_v3_legacy_host_port_in_host_field() {
        let id = Identity::generate();
        // v0.2.6 bug: local_endpoint() passed "ip:port" as host
        let qr = id.qr_payload_with_endpoint(Some("10.0.30.101:9473"), 9473);
        let parsed = parse_qr_payload(&qr).unwrap();
        assert_eq!(parsed.endpoint.as_deref(), Some("10.0.30.101:9473"));
    }

    #[test]
    fn qr_v3_user_reported_payload() {
        let qr = "A04h7pf673PM-S2B0PQSefFdtTOJuPTteIfn800Lvm2GEDEwLjAuMzAuMTAxOjk0NzMlAQ";
        let parsed = parse_qr_payload(qr).unwrap();
        assert_eq!(parsed.endpoint.as_deref(), Some("10.0.30.101:9473"));
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