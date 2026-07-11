//! Per-peer cryptographic state machine.

use x25519_dalek::PublicKey;
use zeroize::Zeroizing;

use super::handshake::HybridKeyExchange;
use super::identity::{compute_sas_with_transcript, Identity, IdentityError};
use super::ratchet::{RATCHET_PK_LEN, SessionRatchet};
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
    pub ratchet: Option<SessionRatchet>,
    transcript: HandshakeTranscript,
    shared_secret: Option<Zeroizing<Vec<u8>>>,
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

    pub fn is_initiator(&self) -> bool {
        self.ratchet.as_ref().is_some_and(|r| r.is_initiator())
    }

    pub fn can_send_encrypted(&self) -> bool {
        self.ratchet.as_ref().is_some_and(|r| r.can_send())
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

        // initiator_finish only needs X25519||CT (32+1088); trailing ratchet pk (and
        // legacy unused ML-KEM EK) are stripped first.
        let kex_body = strip_ratchet_pk_for_kex(&remote_frame.body)?;
        kex.initiator_finish(&kex_body)
            .map_err(|e| e.to_string())?;

        let secret = kex
            .take_shared_secret()
            .ok_or_else(|| "handshake incomplete".to_string())?;
        self.shared_secret = Some(Zeroizing::new(secret.to_vec()));

        let bob_ratchet_pk = extract_bob_ratchet_pk(&remote_frame.body)?;
        let ratchet = SessionRatchet::init_initiator(&secret, &bob_ratchet_pk);
        self.ratchet = Some(ratchet);
        // Step 3: signed transcript completion marker (no extra DH leg).
        Ok(vec![0x01])
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

        let secret = kex
            .shared_secret()
            .ok_or_else(|| "responder handshake incomplete".to_string())?;
        self.shared_secret = Some(Zeroizing::new(secret.to_vec()));

        let (ratchet, bob_pk) =
            SessionRatchet::init_responder(secret).map_err(|e| e.to_string())?;
        self.ratchet = Some(ratchet);
        let mut full_body = resp_body;
        full_body.extend_from_slice(bob_pk.as_bytes());

        self.transcript.append_body(2, &full_body)?;
        let resp = Self::sign_handshake(identity, 2, full_body);
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

        if self.ratchet.is_none() {
            return Err("responder ratchet not initialized".into());
        }

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
        if !ratchet.can_send() {
            return Err(
                "ratchet send chain not ready — wait for peer's first message (responder role)"
                    .into(),
            );
        }
        ratchet
            .encrypt_to_bytes(plaintext)
            .map_err(|e| e.to_string())
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        if !matches!(self.trust, TrustState::SasPending { .. } | TrustState::Trusted) {
            return Err("cannot decrypt — handshake incomplete".into());
        }
        let ratchet = self
            .ratchet
            .as_mut()
            .ok_or_else(|| "no session ratchet".to_string())?;
        ratchet
            .decrypt_from_bytes(ciphertext)
            .map_err(|e| e.to_string())
    }
}

fn extract_bob_ratchet_pk(resp_body: &[u8]) -> Result<PublicKey, String> {
    if resp_body.len() < RATCHET_PK_LEN {
        return Err("handshake response missing ratchet pubkey".into());
    }
    let start = resp_body.len() - RATCHET_PK_LEN;
    Ok(PublicKey::from(
        <[u8; 32]>::try_from(&resp_body[start..])
            .map_err(|_| "invalid ratchet DH key".to_string())?,
    ))
}

/// Body for hybrid KEX finish: X25519(32) + ML-KEM CT(1088), without trailing fields.
///
/// Wire step-2 body layouts:
/// - v0.3+: 32 + 1088 + ratchet_pk(32)
/// - legacy: 32 + 1088 + unused_ek(1184) + ratchet_pk(32)
fn strip_ratchet_pk_for_kex(resp_body: &[u8]) -> Result<Vec<u8>, String> {
    const KEX_LEN: usize = 32 + 1088;
    if resp_body.len() < KEX_LEN + RATCHET_PK_LEN {
        return Err("handshake response too short for KEX + ratchet".into());
    }
    // Always take the fixed KEX prefix; trailing ratchet (and legacy EK) ignored here.
    Ok(resp_body[..KEX_LEN].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ratchet::SessionRatchet;

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

    #[test]
    fn responder_cannot_send_before_first_inbound() {
        let secret = [7u8; 32];
        let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
        let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);

        let env = alice.encrypt(b"hello").unwrap();
        assert!(bob.encrypt(b"too early").is_err());
        let _ = bob.decrypt(&env).unwrap();
        assert!(bob.encrypt(b"reply").is_ok());
    }
}