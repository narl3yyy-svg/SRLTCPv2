//! Wire dump test — application plaintext must not appear on the wire.

use srltcp_core::crypto::identity::Identity;
use srltcp_core::crypto::peer_crypto::PeerCrypto;
use srltcp_core::crypto::wire::{EncryptedPayload, WireFrame};
use srltcp_core::protocol::{ChatMessage, MessageType};

fn complete_handshake(alice: &Identity, bob: &Identity) -> (PeerCrypto, PeerCrypto) {
    let mut alice_kex = srltcp_core::crypto::HybridKeyExchange::initiator();
    let msg1 = alice_kex.initiator_message();
    let frame1 = PeerCrypto::sign_handshake(alice, 1, msg1.clone());

    let mut bob_crypto = PeerCrypto::new_connected();
    let (frame2, _) = bob_crypto
        .responder_process_step1(bob, &frame1)
        .expect("bob step1");

    let mut alice_crypto = PeerCrypto::new_connected();
    alice_crypto.record_initiator_step1(&msg1).expect("alice step1 record");
    let step3_body = alice_crypto
        .initiator_process_step2(&mut alice_kex, &frame2, &bob.public_key_bytes())
        .expect("alice step2");
    let frame3 = PeerCrypto::sign_handshake(alice, 3, step3_body.clone());
    alice_crypto
        .initiator_finalize_step3(alice, &step3_body)
        .expect("alice sas");
    bob_crypto
        .responder_process_step3(bob, &frame3)
        .expect("bob step3");

    bob_crypto.confirm_trusted().expect("bob trust");
    alice_crypto.confirm_trusted().expect("alice trust");

    (alice_crypto, bob_crypto)
}

#[test]
fn encrypted_wire_frame_contains_no_plaintext() {
    let alice = Identity::generate();
    let bob = Identity::generate();
    let (mut alice_crypto, _) = complete_handshake(&alice, &bob);

    let secret_message = "SRLTCP wire dump test — this must never appear on the wire";
    let chat = ChatMessage {
        id: uuid::Uuid::new_v4(),
        sender_id: alice.public_key_hex(),
        recipient_id: bob.public_key_hex(),
        msg_type: MessageType::Text,
        content: secret_message.to_string(),
        timestamp: chrono::Utc::now(),
        metadata: None,
    };
    let plaintext = chat.to_json().expect("json");
    let ciphertext = alice_crypto.encrypt(&plaintext).expect("encrypt");

    let frame = WireFrame::Encrypted(EncryptedPayload {
        version: 3,
        ciphertext,
    });
    let wire = frame.serialize().expect("serialize");

    assert!(
        !wire.windows(secret_message.len()).any(|w| w == secret_message.as_bytes()),
        "plaintext leaked onto wire"
    );
    assert!(
        !wire.windows(plaintext.len()).any(|w| w == plaintext.as_slice()),
        "JSON plaintext leaked onto wire"
    );
    assert!(wire.starts_with(b"SR"), "expected postcard SR magic prefix");
}