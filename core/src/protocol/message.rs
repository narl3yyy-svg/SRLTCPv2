//! Application-level message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MessageType {
    Text,
    File,
    Image,
    Video,
    Audio,
    Folder,
    CallOffer,
    CallAnswer,
    CallIce,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub sender_id: String,
    pub recipient_id: String,
    pub msg_type: MessageType,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub metadata: Option<serde_json::Value>,
}

impl ChatMessage {
    pub fn text(sender: &str, recipient: &str, content: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            sender_id: sender.to_string(),
            recipient_id: recipient.to_string(),
            msg_type: MessageType::Text,
            content: content.to_string(),
            timestamp: Utc::now(),
            metadata: None,
        }
    }

    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_json(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}

/// Wire envelope for all transports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u8,
    pub transport: u8,
    pub encrypted: bool,
    pub payload: Vec<u8>,
}

impl Envelope {
    pub const VERSION: u8 = 2;

    pub fn new(payload: Vec<u8>, encrypted: bool) -> Self {
        Self {
            version: Self::VERSION,
            transport: 0,
            encrypted,
            payload,
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}