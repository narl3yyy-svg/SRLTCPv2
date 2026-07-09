use criterion::{black_box, criterion_group, criterion_main, Criterion};
use srltcp_core::crypto::SessionRatchet;

fn bench_ratchet(c: &mut Criterion) {
    let secret = [7u8; 32];
    let (mut bob, bob_pk) = SessionRatchet::init_responder(&secret).unwrap();
    let mut alice = SessionRatchet::init_initiator(&secret, &bob_pk);
    let payload = b"SRLTCP benchmark message payload for double-ratchet-2";

    c.bench_function("ratchet_encrypt", |b| {
        b.iter(|| {
            let env = alice.encrypt(black_box(payload)).unwrap();
            black_box(env);
        });
    });

    let env = alice.encrypt(payload).unwrap();
    c.bench_function("ratchet_decrypt", |b| {
        b.iter(|| {
            let pt = bob.decrypt(black_box(&env)).unwrap();
            black_box(pt);
        });
    });
}

criterion_group!(benches, bench_ratchet);
criterion_main!(benches);