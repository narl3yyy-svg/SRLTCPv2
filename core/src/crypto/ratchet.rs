//! Signal-spec Double Ratchet via double-ratchet-2.

use double_ratchet_2::header::Header;
use double_ratchet_2::ratchet::Ratchet;
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};

pub const RATCHET_PK_LEN: usize = 32;

#[derive(Debug, Error)]
pub enum RatchetError {
    #[error("decryption failed")]
    DecryptFailed,
    #[error("invalid state")]
    InvalidState,
    #[error("encode error: {0}")]
    Encode(String),
}

/// Serialized ratchet ciphertext on the wire (v0.2.13+).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RatchetEnvelope {
    pub header_bytes: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 12],
}

/// Application-layer Double Ratchet session.
pub struct SessionRatchet {
    inner: Ratchet<StaticSecret>,
    is_initiator: bool,
    /// Initiator may send immediately; responder must receive one message first (Signal spec).
    send_ready: bool,
    /// Bob's ratchet DH public key (init_bob output) — sent in handshake step 2.
    bob_ratchet_pk: Option<PublicKey>,
}

impl SessionRatchet {
    /// Responder path: init_bob; returns ratchet DH pubkey to embed in handshake step 2.
    pub fn init_responder(shared_secret: &[u8]) -> Result<(Self, PublicKey), RatchetError> {
        let sk = derive_ratchet_sk(shared_secret);
        let (inner, bob_pk) = Ratchet::<StaticSecret>::init_bob(sk);
        Ok((
            Self {
                inner,
                is_initiator: false,
                send_ready: false,
                bob_ratchet_pk: Some(bob_pk),
            },
            bob_pk,
        ))
    }

    /// Initiator path: init_alice with bob's ratchet DH pubkey from step 2.
    pub fn init_initiator(shared_secret: &[u8], bob_ratchet_pk: &PublicKey) -> Self {
        let sk = derive_ratchet_sk(shared_secret);
        let inner = Ratchet::<StaticSecret>::init_alice(sk, *bob_ratchet_pk);
        Self {
            inner,
            is_initiator: true,
            send_ready: true,
            bob_ratchet_pk: None,
        }
    }

    pub fn is_initiator(&self) -> bool {
        self.is_initiator
    }

    pub fn can_send(&self) -> bool {
        self.send_ready
    }

    pub fn bob_ratchet_pk(&self) -> Option<&PublicKey> {
        self.bob_ratchet_pk.as_ref()
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<RatchetEnvelope, RatchetError> {
        if !self.send_ready {
            return Err(RatchetError::InvalidState);
        }
        let ad = b"srltcp-v3";
        let (header, ciphertext, nonce) = self.inner.ratchet_encrypt(plaintext, ad);
        Ok(RatchetEnvelope {
            header_bytes: header.concat(ad),
            ciphertext,
            nonce,
        })
    }

    /// Decrypt one message. Corrupt, forged or replayed input returns an error and leaves the
    /// session state untouched.
    ///
    /// `double-ratchet-2` panics (`expect`/`unwrap`) on authentication failure, malformed
    /// headers and too many skipped keys, and mutates its chains before the AEAD check. We
    /// snapshot the state, contain the panic, and roll back — otherwise a single bad frame from
    /// a peer would abort the process or desynchronise the session.
    pub fn decrypt(&mut self, envelope: &RatchetEnvelope) -> Result<Vec<u8>, RatchetError> {
        let ad = b"srltcp-v3";
        let snapshot =
            bincode::serialize(&self.inner).map_err(|e| RatchetError::Encode(e.to_string()))?;
        let result = quiet_catch_unwind(|| {
            let header = Header::<PublicKey>::from(envelope.header_bytes.as_slice());
            self.inner
                .ratchet_decrypt(&header, &envelope.ciphertext, &envelope.nonce, ad)
        });
        match result {
            Ok(plaintext) => {
                if !self.is_initiator {
                    self.send_ready = true;
                }
                Ok(plaintext)
            }
            Err(()) => {
                self.inner = bincode::deserialize(&snapshot).map_err(|_| RatchetError::InvalidState)?;
                Err(RatchetError::DecryptFailed)
            }
        }
    }

    /// Legacy bytes wrapper for wire migration: postcard-encoded envelope.
    pub fn encrypt_to_bytes(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, RatchetError> {
        let env = self.encrypt(plaintext)?;
        postcard::to_allocvec(&env).map_err(|e| RatchetError::Encode(e.to_string()))
    }

    pub fn decrypt_from_bytes(&mut self, data: &[u8]) -> Result<Vec<u8>, RatchetError> {
        let env: RatchetEnvelope = postcard::from_bytes(data)
            .map_err(|_| RatchetError::DecryptFailed)?;
        self.decrypt(&env)
    }
}

thread_local! {
    static SUPPRESS_PANIC_OUTPUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `f`, turning a panic into `Err(())` without printing the panic message for it.
fn quiet_catch_unwind<R>(f: impl FnOnce() -> R) -> Result<R, ()> {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !SUPPRESS_PANIC_OUTPUT.with(|s| s.get()) {
                default(info);
            }
        }));
    });
    SUPPRESS_PANIC_OUTPUT.with(|s| s.set(true));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    SUPPRESS_PANIC_OUTPUT.with(|s| s.set(false));
    out.map_err(|_| ())
}

fn derive_ratchet_sk(shared_secret: &[u8]) -> [u8; 32] {
    let mut okm = [0u8; 32];
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, shared_secret);
    hk.expand(b"srltcp-v3-ratchet-root", &mut okm)
        .expect("HKDF expand");
    okm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_style_roundtrip() {
        let secret = [9u8; 32];
        let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
        let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);

        let pt = b"test message for ratchet";
        let env = alice.encrypt(pt).unwrap();
        let dec = bob.decrypt(&env).unwrap();
        assert_eq!(dec, pt);

        let (env2, _, _) = bob.inner.ratchet_encrypt(b"reply", b"srltcp-v3");
        let header = Header::<PublicKey>::from(env2.concat(b"srltcp-v3").as_slice());
        // bob can encrypt after receiving first message
        let _ = env2;
        let env_a = alice.encrypt(b"second").unwrap();
        let _ = bob.decrypt(&env_a).unwrap();
        let _ = header;
    }

    #[test]
    fn tampered_ciphertext_is_rejected_without_breaking_session() {
        let secret = [3u8; 32];
        let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
        let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);

        let mut bad = alice.encrypt(b"first").unwrap();
        bad.ciphertext[0] ^= 0xff;
        assert!(bob.decrypt(&bad).is_err());
        assert!(!bob.can_send(), "failed decrypt must not open the responder send chain");

        // Garbage header bytes must not panic either.
        let junk = RatchetEnvelope { header_bytes: vec![1, 2, 3], ciphertext: vec![0; 40], nonce: [0; 12] };
        assert!(bob.decrypt(&junk).is_err());
        assert!(bob.decrypt_from_bytes(b"not postcard").is_err());

        // Session still works afterwards.
        let good = alice.encrypt(b"second").unwrap();
        assert_eq!(bob.decrypt(&good).unwrap(), b"second");
        assert!(bob.can_send());
    }

    #[test]
    fn replayed_message_is_rejected() {
        let secret = [4u8; 32];
        let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
        let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);
        let env = alice.encrypt(b"once").unwrap();
        assert_eq!(bob.decrypt(&env).unwrap(), b"once");
        assert!(bob.decrypt(&env).is_err(), "replay must fail");
        let next = alice.encrypt(b"twice").unwrap();
        assert_eq!(bob.decrypt(&next).unwrap(), b"twice");
    }
}