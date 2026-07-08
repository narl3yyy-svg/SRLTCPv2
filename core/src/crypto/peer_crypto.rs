//! Per-peer cryptographic state machine.

use x25519_dalek::PublicKey;

use super::handshake::HybridKeyExchange;
use super::identity::{compute_sas_with_transcript, Identity, IdentityError};
use super::ratchet::DoubleRatchet;
use super::wire::{HandshakeTranscript, SignedHandshake};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustState {
    Connected,
    Handshaking,
    SasPending { sas: String },
    Trusted,
}

pub struct PeerCrypto {
    pub remote_identity: [u8; 32],
    pub trust: TrustState,
    pub ratchet: Option<DoubleRatchet>,
    transcript: HandshakeTranscript,
    shared_secret: Option<Vec<u8>>,
}

impl PeerCrypto {
    pub fn new_connected() -> Self {
        Self {
            remote_identity: [0u8; 32],
            trust: TrustState::Connected,
            ratchet: None,
            transcript: HandshakeTranscript::default(),
            shared_secret: None,
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

    /// Initiator: step 1 body (unsigned placeholder — caller signs).
    pub fn begin_initiator(&mut self) -> (Vec<u8>, HybridKeyExchange) {
        self.trust = TrustState::Handshaking;
        let kex = HybridKeyExchange::initiator();
        let body = kex.initiator_message();
        (body, kex)
    }

    /// Initiator records step 1 after signing.
    pub fn record_initiator_step1(&mut self, body: &[u8]) -> Result<(), String> {
        self.transcript.append_body(1, body)
    }

    /// Initiator processes step 2; returns step-3 DH body bytes (unsigned).
    pub fn initiator_process_step2(
        &mut self,
        kex: &mut HybridKeyExchange,
        remote_frame: &SignedHandshake,
        expected_remote: &[u8; 32],
    ) -> Result<Vec<u8>, String> {
        Self::verify_handshake_frame(remote_frame).map_err(|e| e.to_string())?;
        if remote_frame.step != 2 {
            return Err(format!("expected handshake step 2, got {}", remote_frame.step));
        }
        if remote_frame.identity != *expected_remote {
            return Err("handshake identity does not match QR public key".into());
        }
        self.remote_identity = remote_frame.identity;
        self.transcript.append_body(2, &remote_frame.body)?;

        kex.initiator_finish(&remote_frame.body)
            .map_err(|e| e.to_string())?;

        let secret = kex
            .shared_secret()
            .ok_or_else(|| "handshake incomplete".to_string())?
            .to_vec();
        self.shared_secret = Some(secret);

        let remote_dh = extract_dh_public(&remote_frame.body)?;
        let ratchet = DoubleRatchet::init_sender(
            self.shared_secret.as_ref().unwrap(),
            &remote_dh,
        );
        let step3_body = ratchet
            .dh_public_key()
            .ok_or_else(|| "missing ratchet DH key".to_string())?
            .as_bytes()
            .to_vec();
        self.ratchet = Some(ratchet);
        Ok(step3_body)
    }

    /// Initiator records step 3 and computes SAS (same moment as responder).
    pub fn initiator_finalize_step3(
        &mut self,
        identity: &Identity,
        step3_body: &[u8],
    ) -> Result<String, String> {
        self.transcript.append_body(3, step3_body)?;
        self.finalize_sas(identity)
    }

    /// Responder processes step 1; returns signed step 2 frame.
    pub fn responder_process_step1(
        &mut self,
        identity: &Identity,
        init_frame: &SignedHandshake,
    ) -> Result<(SignedHandshake, HybridKeyExchange), String> {
        Self::verify_handshake_frame(init_frame).map_err(|e| e.to_string())?;
        if init_frame.step != 1 {
            return Err(format!("expected handshake step 1, got {}", init_frame.step));
        }
        self.trust = TrustState::Handshaking;
        self.remote_identity = init_frame.identity;
        self.transcript.append_body(1, &init_frame.body)?;

        let mut kex = HybridKeyExchange::responder();
        let resp_body = kex
            .responder_accept(&init_frame.body)
            .map_err(|e| e.to_string())?;

        self.transcript.append_body(2, &resp_body)?;
        let secret = kex
            .shared_secret()
            .ok_or_else(|| "responder handshake incomplete".to_string())?
            .to_vec();
        self.shared_secret = Some(secret);

        let resp = Self::sign_handshake(identity, 2, resp_body);
        Ok((resp, kex))
    }

    /// Responder processes step 3 and computes SAS.
    pub fn responder_process_step3(
        &mut self,
        identity: &Identity,
        finish_frame: &SignedHandshake,
    ) -> Result<String, String> {
        Self::verify_handshake_frame(finish_frame).map_err(|e| e.to_string())?;
        if finish_frame.step != 3 {
            return Err(format!("expected handshake step 3, got {}", finish_frame.step));
        }
        if finish_frame.identity != self.remote_identity {
            return Err("finish identity mismatch".into());
        }
        self.transcript.append_body(3, &finish_frame.body)?;

        let secret = self
            .shared_secret
            .as_ref()
            .ok_or_else(|| "no shared secret".to_string())?;

        let dh_remote = PublicKey::from(
            <[u8; 32]>::try_from(finish_frame.body.as_slice())
                .map_err(|_| "invalid ratchet DH key".to_string())?,
        );

        let mut ratchet = DoubleRatchet::init_receiver(secret);
        ratchet.dh_ratchet_step(&dh_remote);
        self.ratchet = Some(ratchet);

        self.finalize_sas(identity)
    }

    fn finalize_sas(&mut self, identity: &Identity) -> Result<String, String> {
        if !self.transcript.is_complete() {
            return Err("handshake transcript incomplete".into());
        }
        let secret = self
            .shared_secret
            .as_ref()
            .ok_or_else(|| "no shared secret".to_string())?;
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
        if !matches!(self.trust, TrustState::SasPending { .. } | TrustState::Trusted) {
            return Err("cannot decrypt — handshake incomplete".into());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sas_matches_both_sides_with_canonical_transcript() {
        let alice = Identity::generate();
        let bob = Identity::generate();

        // Simulate wire handshake bodies
        let mut alice_kex = HybridKeyExchange::initiator();
        let msg1 = alice_kex.initiator_message();
        let frame1 = PeerCrypto::sign_handshake(&alice, 1, msg1.clone());

        let mut bob_crypto = PeerCrypto::new_connected();
        let (frame2, _bob_kex) = bob_crypto
            .responder_process_step1(&bob, &frame1)
            .unwrap();

        let mut alice_crypto = PeerCrypto::new_connected();
        alice_crypto.record_initiator_step1(&msg1).unwrap();
        let step3_body = alice_crypto
            .initiator_process_step2(&mut alice_kex, &frame2, &bob.public_key_bytes())
            .unwrap();
        let frame3 = PeerCrypto::sign_handshake(&alice, 3, step3_body.clone());
        let sas_alice = alice_crypto
            .initiator_finalize_step3(&alice, &step3_body)
            .unwrap();

        let sas_bob = bob_crypto.responder_process_step3(&bob, &frame3).unwrap();
        assert_eq!(sas_alice, sas_bob);
        assert_eq!(sas_alice.len(), 6);
    }
}