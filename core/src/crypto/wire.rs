//! Wire framing for handshake control and encrypted application data.

use serde::{Deserialize, Serialize};

/// Top-level wire frame sent over QUIC / serial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireFrame {
    /// Hybrid KEX handshake (signed with Ed25519 long-term identity).
    Handshake(SignedHandshake),
    /// Double-ratchet ciphertext.
    Encrypted(EncryptedPayload),
}

/// Signed hybrid handshake step (1 = initiator, 2 = responder, 3 = ratchet DH).
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

impl WireFrame {
    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
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