//! Call signaling types for voice/video.
//!
//! **Security note (honest):** application messages (SDP / ICE / hangup) are
//! relayed as encrypted Double Ratchet payloads over iroh. **Media itself is not
//! Double-Ratchet wrapped** — platform WebRTC uses STUN + DTLS-SRTP. Treat call
//! content confidentiality as standard WebRTC transport security, not full
//! app-layer E2EE.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum WebRtcError {
    #[error("call not active")]
    NotActive,
    #[error("signaling error: {0}")]
    Signaling(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CallState {
    Idle,
    Offering,
    Ringing,
    Connected,
    Ended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
}

/// Lightweight call session metadata (media is owned by platform WebRTC stacks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSession {
    pub id: String,
    pub peer_id: String,
    pub state: CallState,
    pub is_video: bool,
    pub local_sdp: Option<String>,
    pub remote_sdp: Option<String>,
}

impl CallSession {
    pub fn new(peer_id: &str, is_video: bool) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            peer_id: peer_id.to_string(),
            state: CallState::Idle,
            is_video,
            local_sdp: None,
            remote_sdp: None,
        }
    }

    pub fn mark_offering(&mut self) {
        self.state = CallState::Offering;
        info!(call_id = %self.id, video = self.is_video, "call offering");
    }

    pub fn accept_answer(&mut self, sdp: &str) -> Result<(), WebRtcError> {
        self.remote_sdp = Some(sdp.to_string());
        self.state = CallState::Connected;
        info!(call_id = %self.id, "call connected (signaling)");
        Ok(())
    }

    pub fn end(&mut self) {
        self.state = CallState::Ended;
        info!(call_id = %self.id, "call ended");
    }
}
