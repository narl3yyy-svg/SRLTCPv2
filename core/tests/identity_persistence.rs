//! Long-term identity must be stable across engine restarts.

use srltcp_core::crypto::{load_or_create_seed_file, Identity, IdentitySeed};
use srltcp_core::p2p::P2pEngine;

#[test]
fn seed_roundtrip_same_public_key() {
    let seed = IdentitySeed::generate();
    let a = Identity::from_seed(&seed);
    let b = Identity::from_seed(&IdentitySeed::from_hex(&seed.to_hex()).unwrap());
    assert_eq!(a.public_key_bytes(), b.public_key_bytes());
    assert_eq!(a.public_key_hex(), b.public_key_hex());
}

#[test]
fn engine_with_identity_preserves_key() {
    let seed = IdentitySeed::generate();
    let id = Identity::from_seed(&seed);
    let pk = id.public_key_hex();
    let (engine, _rx) = P2pEngine::with_identity(id);
    assert_eq!(engine.public_key_hex(), pk);

    let id2 = Identity::from_seed(&seed);
    let (engine2, _rx2) = P2pEngine::with_identity(id2);
    assert_eq!(engine2.public_key_hex(), pk);
}

#[test]
fn seed_file_survives_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id.seed");
    let s1 = load_or_create_seed_file(&path).unwrap();
    let pk1 = Identity::from_seed(&s1).public_key_hex();
    let s2 = load_or_create_seed_file(&path).unwrap();
    let pk2 = Identity::from_seed(&s2).public_key_hex();
    assert_eq!(pk1, pk2);
}
