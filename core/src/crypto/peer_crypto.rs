//! Per-peer cryptographic state machine.

use x25519_dalek::PublicKey;

use super::handshake::HybridKeyExchange;
use super::identity::{compute_sas_with_transcript, Identity, IdentityError};
use super::ratchet::DoubleRatchet;
use super::wire::{HandshakeTranscript, SignedHandshake};

/// Trust lifecycle for a peer connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustState {
    Connected,
    Handshaking,
    SasPending { sas: String },
    Trusted,
}

/// Cryptographic material for an established peer.
pub struct PeerCrypto {
    pub remote_identity: [u8; 32],
    pub trust: TrustState,
    pub ratchet: Option<DoubleRatchet>,
    transcript: HandshakeTranscript,
}

impl PeerCrypto {
    pub fn new_connected() -> Self {
        Self {
            remote_identity: [0u8; 32],
            trust: TrustState::Connected,
            ratchet: None,
            transcript: HandshakeTranscript::default(),
        }
    }

    pub fn is_trusted(&self) -> bool {
        self.trust == TrustState::Trusted
    }

    pub fn sas_pending(&self) -> Option<&str> {
        match &self.trust {
            TrustState::SasPending { sas } => Some(sas.as_str()),
            _ => None,
        }
    }

    pub fn confirm_trusted(&mut self) -> Result<(), String> {
        match &self.trust {
            TrustState::SasPending { .. } => {
                self.trust = TrustState::Trusted;
                Ok(())
            }
            TrustState::Trusted => Ok(()),
            _ => Err("peer is not awaiting SAS confirmation".into()),
        }
    }

    pub fn sign_handshake(identity: &Identity, step: u8, body: Vec<u8>) -> SignedHandshake {
        let id_pk = identity.public_key_bytes();
        let mut sign_input = Vec::with_capacity(1 + 32 + body.len());
        sign_input.push(step);
        sign_input.extend_from_slice(&id_pk);
        sign_input.extend_from_slice(&body);
        let signature = identity.sign(&sign_input);
        SignedHandshake {
            step,
            identity: id_pk,
            body,
            signature: signature.to_vec(),
        }
    }

    pub fn verify_handshake_frame(frame: &SignedHandshake) -> Result<(), IdentityError> {
        let mut sign_input = Vec::with_capacity(1 + 32 + frame.body.len());
        sign_input.push(frame.step);
        sign_input.extend_from_slice(&frame.identity);
        sign_input.extend_from_slice(&frame.body);
        let sig: [u8; 64] = frame
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| IdentityError::InvalidKey("expected 64-byte signature".into()))?;
        Identity::verify(&frame.identity, &sign_input, &sig)
    }

    pub fn begin_initiator(&mut self) -> (SignedHandshake, HybridKeyExchange) {
        self.trust = TrustState::Handshaking;
        let kex = HybridKeyExchange::initiator();
        let body = kex.initiator_message();
        (Self::sign_handshake_placeholder(1, body), kex)
    }

    fn sign_handshake_placeholder(step: u8, body: Vec<u8>) -> SignedHandshake {
        SignedHandshake {
            step,
            identity: [0u8; 32],
            body,
            signature: Vec::new(),
        }
    }

    pub fn initiator_finish(
        &mut self,
        identity: &Identity,
        kex: &mut HybridKeyExchange,
        remote_frame: &SignedHandshake,
        expected_remote: &[u8; 32],
    ) -> Result<(SignedHandshake, String), String> {
        Self::verify_handshake_frame(remote_frame).map_err(|e| e.to_string())?;
        if remote_frame.identity != *expected_remote {
            return Err("handshake identity does not match QR public key".into());
        }
        self.remote_identity = remote_frame.identity;
        self.transcript.append(remote_frame);

        kex.initiator_finish(&remote_frame.body)
            .map_err(|e| e.to_string())?;

        let secret = kex
            .shared_secret()
            .ok_or_else(|| "handshake incomplete".to_string())?;

        let remote_dh = extract_dh_public(&remote_frame.body)?;
        let ratchet = DoubleRatchet::init_sender(secret, &remote_dh);
        let dh_pub = ratchet
            .dh_public_key()
            .ok_or_else(|| "missing ratchet DH key".to_string())?;

        let finish = Self::sign_handshake(identity, 3, dh_pub.as_bytes().to_vec());
        self.transcript.append(&finish);

        let sas = compute_sas_with_transcript(
            secret,
            &identity.public_key_bytes(),
            &self.remote_identity,
            self.transcript.as_bytes(),
        );
        self.ratchet = Some(ratchet);
        self.trust = TrustState::SasPending { sas: sas.clone() };
        Ok((finish, sas))
    }

    pub fn responder_accept(
        &mut self,
        identity: &Identity,
        init_frame: &SignedHandshake,
    ) -> Result<(SignedHandshake, HybridKeyExchange), String> {
        Self::verify_handshake_frame(init_frame).map_err(|e| e.to_string())?;
        self.trust = TrustState::Handshaking;
        self.remote_identity = init_frame.identity;
        self.transcript.append(init_frame);

        let mut kex = HybridKeyExchange::responder();
        let resp_body = kex
            .responder_accept(&init_frame.body)
            .map_err(|e| e.to_string())?;

        let resp = Self::sign_handshake(identity, 2, resp_body);
        self.transcript.append(&resp);
        Ok((resp, kex))
    }

    pub fn responder_finish(
        &mut self,
        identity: &Identity,
        kex: &HybridKeyExchange,
        finish_frame: &SignedHandshake,
    ) -> Result<String, String> {
        Self::verify_handshake_frame(finish_frame).map_err(|e| e.to_string())?;
        if finish_frame.identity != self.remote_identity {
            return Err("finish identity mismatch".into());
        }
        self.transcript.append(finish_frame);

        let secret = kex
            .shared_secret()
            .ok_or_else(|| "responder handshake incomplete".to_string())?;

        let dh_remote = PublicKey::from(
            <[u8; 32]>::try_from(finish_frame.body.as_slice())
                .map_err(|_| "invalid ratchet DH key".to_string())?,
        );

        let mut ratchet = DoubleRatchet::init_receiver(secret);
        ratchet.dh_ratchet_step(&dh_remote);
        self.ratchet = Some(ratchet);

        let sas = compute_sas_with_transcript(
            secret,
            &identity.public_key_bytes(),
            &self.remote_identity,
            self.transcript.as_bytes(),
        );
        self.trust = TrustState::SasPending { sas: sas.clone() };
        Ok(sas)
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        if !self.is_trusted() {
            return Err("peer not trusted — confirm SAS first".into());
        }
        let ratchet = self
            .ratchet
            .as_mut()
            .ok_or_else(|| "no session ratchet".to_string())?;
        ratchet.encrypt(plaintext).map_err(|e| e.to_string())
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        let ratchet = self
            .ratchet
            .as_mut()
            .ok_or_else(|| "no session ratchet".to_string())?;
        ratchet.decrypt(ciphertext).map_err(|e| e.to_string())
    }
}

fn extract_dh_public(resp_body: &[u8]) -> Result<PublicKey, String> {
    if resp_body.len() < 32 {
        return Err("handshake response too short".into());
    }
    Ok(PublicKey::from(
        <[u8; 32]>::try_from(&resp_body[..32]).map_err(|_| "invalid x25519 key".to_string())?,
    ))
}