//! Wire framing for handshake control and encrypted application data.

use serde::{Deserialize, Serialize};

/// Top-level wire frame sent over QUIC / serial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireFrame {
    /// Hybrid KEX handshake (signed with Ed25519 long-term identity).
    Handshake(SignedHandshake),
    /// Double-ratchet ciphertext inside an envelope shell.
    Encrypted(EncryptedPayload),
}

/// Signed hybrid handshake step (1 = initiator, 2 = responder, 3 = initiator finish).
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

/// Accumulates handshake bytes for SAS binding.
#[derive(Debug, Default, Clone)]
pub struct HandshakeTranscript {
    bytes: Vec<u8>,
}

impl HandshakeTranscript {
    pub fn append(&mut self, frame: &SignedHandshake) {
        self.bytes.push(frame.step);
        self.bytes.extend_from_slice(&frame.identity);
        self.bytes.extend_from_slice(&frame.body);
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}