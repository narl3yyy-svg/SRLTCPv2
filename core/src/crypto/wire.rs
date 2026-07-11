//! Wire framing for handshake control and encrypted application data.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Magic bytes prefix for postcard-encoded frames (v0.2.12+).
pub const WIRE_MAGIC: &[u8; 2] = b"SR";

/// Top-level wire frame sent over QUIC / serial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireFrame {
    /// Hybrid KEX handshake (signed with Ed25519 long-term identity).
    Handshake(SignedHandshake),
    /// Double-ratchet ciphertext.
    Encrypted(EncryptedPayload),
}

/// Signed hybrid handshake step (1 = initiator, 2 = responder, 3 = transcript complete).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedHandshake {
    pub step: u8,
    pub identity: [u8; 32],
    pub body: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPayload {
    pub version: u8,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum WireError {
    #[error("postcard encode: {0}")]
    Encode(String),
    #[error("postcard decode: {0}")]
    Decode(String),
    #[error("json decode: {0}")]
    Json(#[from] serde_json::Error),
}

impl WireFrame {
    /// Serialize using compact postcard format with SR magic prefix.
    pub fn serialize(&self) -> Result<Vec<u8>, WireError> {
        let payload = postcard::to_allocvec(self).map_err(|e| WireError::Encode(e.to_string()))?;
        let mut out = Vec::with_capacity(WIRE_MAGIC.len() + payload.len());
        out.extend_from_slice(WIRE_MAGIC);
        out.extend_from_slice(&payload);
        Ok(out)
    }

    /// Deserialize postcard (SR prefix) or legacy JSON frames.
    pub fn deserialize(data: &[u8]) -> Result<Self, WireError> {
        if data.len() >= WIRE_MAGIC.len() && data.starts_with(WIRE_MAGIC) {
            return postcard::from_bytes(&data[WIRE_MAGIC.len()..])
                .map_err(|e| WireError::Decode(e.to_string()));
        }
        Ok(serde_json::from_slice(data)?)
    }
}

/// Canonical handshake transcript: step bodies 1→2→3 in order (both peers identical).
#[derive(Debug, Clone)]
pub struct HandshakeTranscript {
    bytes: Vec<u8>,
    next_step: u8,
}

impl Default for HandshakeTranscript {
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            next_step: 1,
        }
    }
}

impl HandshakeTranscript {
    /// Append a handshake step body; steps must arrive in order 1, 2, 3.
    pub fn append_body(&mut self, step: u8, body: &[u8]) -> Result<(), String> {
        if step != self.next_step {
            return Err(format!(
                "transcript out of order: expected step {}, got {step}",
                self.next_step
            ));
        }
        self.next_step += 1;
        let len = (body.len() as u32).to_be_bytes();
        self.bytes.extend_from_slice(&len);
        self.bytes.extend_from_slice(body);
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.next_step > 3
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postcard_roundtrip_with_magic_prefix() {
        let frame = WireFrame::Handshake(SignedHandshake {
            step: 1,
            identity: [7u8; 32],
            body: vec![1, 2, 3],
            signature: vec![4, 5],
        });
        let bytes = frame.serialize().expect("serialize");
        assert!(bytes.starts_with(WIRE_MAGIC));
        let decoded = WireFrame::deserialize(&bytes).expect("deserialize");
        match decoded {
            WireFrame::Handshake(hs) => {
                assert_eq!(hs.step, 1);
                assert_eq!(hs.body, vec![1, 2, 3]);
            }
            _ => panic!("expected handshake frame"),
        }
    }

    #[test]
    fn legacy_json_still_deserializes() {
        let json = br#"{"Handshake":{"step":2,"identity":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"body":[],"signature":[]}}"#;
        let decoded = WireFrame::deserialize(json).expect("json deserialize");
        match decoded {
            WireFrame::Handshake(hs) => assert_eq!(hs.step, 2),
            _ => panic!("expected handshake frame"),
        }
    }
}