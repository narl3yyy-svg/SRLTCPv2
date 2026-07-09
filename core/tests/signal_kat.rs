//! Signal-spec Double Ratchet known-answer style tests via SessionRatchet wrapper.
//! Mirrors double-ratchet-2 upstream integration tests.

use srltcp_core::crypto::SessionRatchet;

fn kat_secret() -> [u8; 32] {
    [1u8; 32]
}

#[test]
fn kat_init_bob_alice() {
    let secret = kat_secret();
    let (_bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
    let _alice = SessionRatchet::init_initiator(&secret, &bob_pk);
}

#[test]
fn kat_single_message_roundtrip() {
    let secret = kat_secret();
    let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
    let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);

    let data = include_bytes!("../src/crypto/ratchet.rs");
    let env = alice.encrypt(data).unwrap();
    let dec = bob.decrypt(&env).unwrap();
    assert_eq!(dec, data);
}

#[test]
fn kat_out_of_order_decrypt() {
    let secret = kat_secret();
    let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
    let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);

    let data = include_bytes!("../src/crypto/handshake.rs");
    let e1 = alice.encrypt(data).unwrap();
    let e2 = alice.encrypt(data).unwrap();
    let e3 = alice.encrypt(data).unwrap();

    assert_eq!(bob.decrypt(&e3).unwrap(), data);
    assert_eq!(bob.decrypt(&e2).unwrap(), data);
    assert_eq!(bob.decrypt(&e1).unwrap(), data);
}

#[test]
fn kat_bidirectional_exchange() {
    let secret = kat_secret();
    let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
    let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);

    let data = include_bytes!("../src/crypto/peer_crypto.rs");
    let a1 = alice.encrypt(data).unwrap();
    assert_eq!(bob.decrypt(&a1).unwrap(), data);

    let b1 = bob.encrypt(b"reply from bob").unwrap();
    assert_eq!(alice.decrypt(&b1).unwrap(), b"reply from bob");
}

#[test]
fn kat_bytes_wrapper_roundtrip() {
    let secret = kat_secret();
    let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
    let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);

    let pt = b"postcard envelope path";
    let ct = alice.encrypt_to_bytes(pt).unwrap();
    let dec = bob.decrypt_from_bytes(&ct).unwrap();
    assert_eq!(dec, pt);
}

#[test]
fn hybrid_kex_symmetric_secret() {
    use srltcp_core::crypto::HybridKeyExchange;

    let mut alice_kex = HybridKeyExchange::initiator();
    let msg1 = alice_kex.initiator_message();

    let mut bob_kex = HybridKeyExchange::responder();
    let resp = bob_kex.responder_accept(&msg1).unwrap();
    alice_kex.initiator_finish(&resp).unwrap();

    let alice_secret = alice_kex.shared_secret().unwrap();
    let bob_secret = bob_kex.shared_secret().unwrap();
    assert_eq!(alice_secret, bob_secret);
    assert_eq!(alice_secret.len(), 32);
}