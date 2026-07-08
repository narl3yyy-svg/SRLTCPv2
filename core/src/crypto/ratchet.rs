//! Double Ratchet for forward secrecy and post-compromise security.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;
use x25519_dalek::{EphemeralSecret, PublicKey};

#[derive(Debug, Error)]
pub enum RatchetError {
    #[error("decryption failed")]
    DecryptFailed,
    #[error("invalid state")]
    InvalidState,
}

/// Simplified Double Ratchet session state.
pub struct DoubleRatchet {
    send_chain_key: [u8; 32],
    send_cipher: Aes256Gcm,
    recv_cipher: Aes256Gcm,
    send_count: u32,
    recv_count: u32,
    dh_send_secret: Option<EphemeralSecret>,
    dh_send_public: Option<PublicKey>,
    dh_recv_public: Option<PublicKey>,
    root_key: [u8; 32],
}

impl DoubleRatchet {
    pub fn init_sender(shared_secret: &[u8], remote_dh_public: &PublicKey) -> Self {
        let mut root_key = [0u8; 32];
        let hk = Hkdf::<Sha256>::new(None, shared_secret);
        hk.expand(b"srltcp-v2-ratchet-root", &mut root_key).unwrap();

        let dh_secret = EphemeralSecret::random_from_rng(rand::rngs::OsRng);
        let dh_public = PublicKey::from(&dh_secret);

        let (send_chain, send_cipher) = Self::derive_chain_key(&root_key, b"send");
        let (_recv_chain, recv_cipher) = Self::derive_chain_key(&root_key, b"recv");

        Self {
            send_chain_key: send_chain,
            send_cipher,
            recv_cipher,
            send_count: 0,
            recv_count: 0,
            dh_send_secret: Some(dh_secret),
            dh_send_public: Some(dh_public),
            dh_recv_public: Some(*remote_dh_public),
            root_key,
        }
    }

    pub fn init_receiver(shared_secret: &[u8]) -> Self {
        let mut root_key = [0u8; 32];
        let hk = Hkdf::<Sha256>::new(None, shared_secret);
        hk.expand(b"srltcp-v2-ratchet-root", &mut root_key).unwrap();

        let (send_chain, send_cipher) = Self::derive_chain_key(&root_key, b"recv");
        let (_recv_chain, recv_cipher) = Self::derive_chain_key(&root_key, b"send");

        Self {
            send_chain_key: send_chain,
            send_cipher,
            recv_cipher,
            send_count: 0,
            recv_count: 0,
            dh_send_secret: None,
            dh_send_public: None,
            dh_recv_public: None,
            root_key,
        }
    }

    fn derive_chain_key(root: &[u8; 32], label: &[u8]) -> ([u8; 32], Aes256Gcm) {
        let hk = Hkdf::<Sha256>::new(None, root);
        let mut key = [0u8; 32];
        hk.expand(label, &mut key).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).expect("key");
        (key, cipher)
    }

    fn advance_send_chain(&mut self) {
        let hk = Hkdf::<Sha256>::new(None, &self.send_chain_key);
        let mut next = [0u8; 32];
        hk.expand(b"chain", &mut next).unwrap();
        self.send_chain_key = next;
        self.send_cipher = Aes256Gcm::new_from_slice(&next).expect("key");
        self.send_count = 0;
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, RatchetError> {
        let mut nonce = [0u8; 12];
        nonce[8..].copy_from_slice(&self.send_count.to_be_bytes());
        self.send_count += 1;

        let result = self
            .send_cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|_| RatchetError::DecryptFailed)?;

        // Ratchet every 256 messages
        if self.send_count >= 256 {
            self.advance_send_chain();
        }

        Ok(result)
    }

    /// Root key material (for SAS binding on responder path).
    pub fn root_key(&self) -> &[u8] {
        &self.root_key
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, RatchetError> {
        let mut nonce = [0u8; 12];
        nonce[8..].copy_from_slice(&self.recv_count.to_be_bytes());
        self.recv_count += 1;

        self.recv_cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext)
            .map_err(|_| RatchetError::DecryptFailed)
    }

    pub fn dh_public_key(&self) -> Option<&PublicKey> {
        self.dh_send_public.as_ref()
    }

    /// Perform DH ratchet step when receiving a new remote DH public key.
    pub fn dh_ratchet_step(&mut self, remote_public: &PublicKey) {
        if let Some(secret) = self.dh_send_secret.take() {
            let shared = secret.diffie_hellman(remote_public);
            let hk = Hkdf::<Sha256>::new(Some(&self.root_key), shared.as_bytes());
            let mut new_root = [0u8; 32];
            hk.expand(b"dh-ratchet", &mut new_root).unwrap();
            self.root_key = new_root;

            let (send_chain, send_cipher) = Self::derive_chain_key(&new_root, b"send");
            self.send_chain_key = send_chain;
            self.send_cipher = send_cipher;
            self.send_count = 0;
        }
        self.dh_recv_public = Some(*remote_public);

        // Generate new DH keypair for next send
        let new_secret = EphemeralSecret::random_from_rng(rand::rngs::OsRng);
        let new_public = PublicKey::from(&new_secret);
        self.dh_send_secret = Some(new_secret);
        self.dh_send_public = Some(new_public);
    }
}