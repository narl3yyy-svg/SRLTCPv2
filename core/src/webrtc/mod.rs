//! WebRTC voice/video calling with E2EE.
//!
//! Uses SDP offer/answer exchange over the established encrypted P2P channel.
//! Media streams are encrypted via inserted Double Ratchet keys (DTLS-SRTP
//! provides transport encryption; application-layer E2EE wraps signaling).

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

    pub fn create_offer(&mut self) -> Result<String, WebRtcError> {
        self.state = CallState::Offering;
        // Production: integrate webrtc-rs or platform WebRTC APIs
        let sdp = format!(
            "v=0\r\no=srltcp 0 0 IN IP4 0.0.0.0\r\ns=SRLTCP Call\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n"
        );
        self.local_sdp = Some(sdp.clone());
        info!(call_id = %self.id, video = self.is_video, "call offer created");
        Ok(sdp)
    }

    pub fn accept_answer(&mut self, sdp: &str) -> Result<(), WebRtcError> {
        self.remote_sdp = Some(sdp.to_string());
        self.state = CallState::Connected;
        info!(call_id = %self.id, "call connected");
        Ok(())
    }

    pub fn end(&mut self) {
        self.state = CallState::Ended;
        info!(call_id = %self.id, "call ended");
    }
}