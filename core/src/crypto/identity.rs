//! Long-term identity keys (Ed25519) and QR-based discovery.

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("invalid key material: {0}")]
    InvalidKey(String),
    #[error("signature verification failed")]
    BadSignature,
    #[error("identity has no signing key (verify-only)")]
    NoSigningKey,
}

/// 32-byte Ed25519 seed that is zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct IdentitySeed([u8; 32]);

impl IdentitySeed {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut OsRng, &mut bytes);
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, IdentityError> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| IdentityError::InvalidKey("expected 32-byte seed".into()))?;
        Ok(Self(arr))
    }

    pub fn from_hex(hex_str: &str) -> Result<Self, IdentityError> {
        let clean = hex_str.trim();
        let bytes = hex::decode(clean)
            .map_err(|e| IdentityError::InvalidKey(format!("invalid seed hex: {e}")))?;
        Self::from_slice(&bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// Ed25519 identity for long-term authentication.
#[derive(Clone)]
pub struct Identity {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl Identity {
    pub fn generate() -> Self {
        Self::from_seed(&IdentitySeed::generate())
    }

    pub fn from_seed(seed: &IdentitySeed) -> Self {
        let signing_key = SigningKey::from_bytes(seed.as_bytes());
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Legacy constructor — prefers [`from_seed`].
    pub fn from_seed_bytes(seed: &[u8; 32]) -> Self {
        Self::from_seed(&IdentitySeed::from_bytes(*seed))
    }

    /// Export the 32-byte seed for secure persistence (caller must protect storage).
    pub fn to_seed(&self) -> IdentitySeed {
        IdentitySeed::from_bytes(self.signing_key.to_bytes())
    }

    pub fn seed_hex(&self) -> String {
        self.to_seed().to_hex()
    }

    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key_bytes())
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

    /// QR payload v4 embeds iroh EndpointTicket for NAT traversal (no port forwarding).
    pub fn qr_payload_v4(&self, iroh_ticket: &str) -> String {
        let ticket_bytes = iroh_ticket.as_bytes();
        let ticket_len = ticket_bytes.len().min(4096);
        let mut payload = Vec::with_capacity(35 + ticket_len);
        payload.push(0x04);
        payload.extend_from_slice(&self.public_key_bytes());
        payload.extend_from_slice(&(ticket_len as u16).to_be_bytes());
        payload.extend_from_slice(&ticket_bytes[..ticket_len]);
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            &payload,
        )
    }

    /// QR payload v3 embeds LAN endpoint (legacy — v0.2.12 and earlier).
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
    /// Legacy v3 LAN endpoint (`host:port`) — deprecated in v0.2.13.
    pub endpoint: Option<String>,
    /// iroh EndpointTicket string for NAT traversal (v4).
    pub iroh_ticket: Option<String>,
}

/// Strip whitespace/newlines from pasted QR payloads.
pub fn normalize_qr_input(payload: &str) -> String {
    payload.chars().filter(|c| !c.is_whitespace()).collect()
}

fn decode_qr_base64(payload: &str) -> Result<Vec<u8>, IdentityError> {
    use base64::Engine;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};

    let clean = normalize_qr_input(payload);
    if clean.is_empty() {
        return Err(IdentityError::InvalidKey("empty QR payload".into()));
    }

    URL_SAFE_NO_PAD
        .decode(&clean)
        .or_else(|_| URL_SAFE.decode(&clean))
        .or_else(|_| STANDARD.decode(&clean))
        .map_err(|e| IdentityError::InvalidKey(format!("invalid base64: {e}")))
}

/// Parse v2/v3/v4 QR payloads (identity, optional LAN endpoint, or iroh ticket).
pub fn parse_qr_payload(payload: &str) -> Result<ParsedQr, IdentityError> {
    let decoded = decode_qr_base64(payload)?;

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
                iroh_ticket: None,
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
                iroh_ticket: None,
            })
        }
        0x04 => {
            if decoded.len() < 35 {
                return Err(IdentityError::InvalidKey("invalid v4 QR length".into()));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&decoded[1..33]);
            let ticket_len = u16::from_be_bytes([decoded[33], decoded[34]]) as usize;
            if decoded.len() < 35 + ticket_len {
                return Err(IdentityError::InvalidKey("invalid v4 ticket field".into()));
            }
            let ticket = std::str::from_utf8(&decoded[35..35 + ticket_len])
                .map_err(|e| IdentityError::InvalidKey(e.to_string()))?
                .to_string();
            Ok(ParsedQr {
                public_key: key,
                endpoint: None,
                iroh_ticket: Some(ticket),
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

/// Load-or-create a persistent identity seed from a file (desktop / CLI).
/// File mode is set to 0o600 on Unix. Format: ASCII hex of 32-byte seed + newline.
pub fn load_or_create_seed_file(path: &std::path::Path) -> Result<IdentitySeed, IdentityError> {
    if path.exists() {
        let data = std::fs::read_to_string(path)
            .map_err(|e| IdentityError::InvalidKey(format!("read identity seed: {e}")))?;
        let seed = IdentitySeed::from_hex(data.trim())?;
        return Ok(seed);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| IdentityError::InvalidKey(format!("create identity dir: {e}")))?;
    }
    let seed = IdentitySeed::generate();
    write_seed_file(path, &seed)?;
    Ok(seed)
}

/// Write identity seed to disk (0o600 on Unix).
pub fn write_seed_file(path: &std::path::Path, seed: &IdentitySeed) -> Result<(), IdentityError> {
    let hex = seed.to_hex();
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| IdentityError::InvalidKey(format!("write identity seed: {e}")))?;
        f.write_all(hex.as_bytes())
            .map_err(|e| IdentityError::InvalidKey(format!("write identity seed: {e}")))?;
        f.write_all(b"\n")
            .map_err(|e| IdentityError::InvalidKey(format!("write identity seed: {e}")))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, format!("{hex}\n"))
            .map_err(|e| IdentityError::InvalidKey(format!("write identity seed: {e}")))?;
    }
    // Best-effort wipe of stack hex (String still may linger until drop).
    let _ = Zeroizing::new(hex);
    Ok(())
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
    fn seed_roundtrip_stable_pubkey() {
        let seed = IdentitySeed::generate();
        let a = Identity::from_seed(&seed);
        let b = Identity::from_seed(&seed);
        assert_eq!(a.public_key_bytes(), b.public_key_bytes());
        let hex = seed.to_hex();
        let restored = IdentitySeed::from_hex(&hex).unwrap();
        let c = Identity::from_seed(&restored);
        assert_eq!(a.public_key_bytes(), c.public_key_bytes());
    }

    #[test]
    fn seed_file_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.seed");
        let seed1 = load_or_create_seed_file(&path).unwrap();
        let seed2 = load_or_create_seed_file(&path).unwrap();
        assert_eq!(seed1.as_bytes(), seed2.as_bytes());
        let id1 = Identity::from_seed(&seed1);
        let id2 = Identity::from_seed(&seed2);
        assert_eq!(id1.public_key_hex(), id2.public_key_hex());
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
    fn qr_v4_roundtrip() {
        let id = Identity::generate();
        let ticket = "3bq2aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let qr = id.qr_payload_v4(ticket);
        let parsed = parse_qr_payload(&qr).unwrap();
        assert_eq!(parsed.public_key, id.public_key_bytes());
        assert_eq!(parsed.iroh_ticket.as_deref(), Some(ticket));
        assert!(parsed.endpoint.is_none());
    }

    #[test]
    fn qr_v4_paste_with_whitespace() {
        let id = Identity::generate();
        let ticket = "ticket-with-dashes_and.dots/ok";
        let qr = id.qr_payload_v4(ticket);
        let pasted = format!("  {qr}\n\t ");
        let parsed = parse_qr_payload(&pasted).unwrap();
        assert_eq!(parsed.iroh_ticket.as_deref(), Some(ticket));
    }

    #[test]
    fn qr_v4_rejects_v2_without_ticket() {
        let id = Identity::generate();
        let qr = id.qr_payload();
        let parsed = parse_qr_payload(&qr).unwrap();
        assert!(parsed.iroh_ticket.is_none());
    }

    #[test]
    fn normalize_strips_all_whitespace() {
        assert_eq!(normalize_qr_input(" ab\nc\t"), "abc");
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
