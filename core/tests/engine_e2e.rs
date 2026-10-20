//! Two-peer end-to-end simulation over real iroh endpoints (needs network for relay/DNS).
//!
//! Run with: cargo test -p srltcp-core --test engine_e2e -- --ignored --nocapture

use std::sync::Arc;
use std::time::Duration;

use srltcp_core::p2p::{EngineEvent, P2pEngine};
use tokio::sync::mpsc;

struct Peer {
    engine: Arc<P2pEngine>,
    events: mpsc::Receiver<EngineEvent>,
}

async fn spawn_peer(dir: &std::path::Path) -> Peer {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    let (engine, events) = P2pEngine::new();
    engine.set_receive_dir(dir.to_path_buf()).await;
    engine.start(0).await.expect("start");
    engine.wait_until_ready(60).await.expect("ready");
    Peer {
        engine: Arc::new(engine),
        events,
    }
}

async fn try_wait_for<F>(rx: &mut mpsc::Receiver<EngineEvent>, secs: u64, mut pred: F) -> Option<EngineEvent>
where
    F: FnMut(&EngineEvent) -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let ev = tokio::time::timeout_at(deadline, rx.recv()).await.ok()??;
        if pred(&ev) {
            return Some(ev);
        }
    }
}

async fn wait_for<F>(rx: &mut mpsc::Receiver<EngineEvent>, secs: u64, pred: F) -> EngineEvent
where
    F: FnMut(&EngineEvent) -> bool,
{
    try_wait_for(rx, secs, pred)
        .await
        .expect("timed out waiting for engine event")
}

/// A and B pair (QR + SAS), both confirm, then exchange messages in both directions.
async fn pair(a: &mut Peer, b: &mut Peer) -> (String, String) {
    let b_qr = b.engine.qr_payload_async().await.unwrap();
    let (b_id_on_a, sas_a, auto) = a.engine.connect_and_verify(&b_qr).await.expect("connect");
    assert!(!auto);
    let ev = wait_for(&mut b.events, 30, |e| matches!(e, EngineEvent::SasReady { .. })).await;
    let EngineEvent::SasReady { peer_id: a_id_on_b, sas: sas_b, .. } = ev else { unreachable!() };
    assert_eq!(sas_a, sas_b, "SAS must match on both sides");
    a.engine.confirm_peer_trusted(&b_id_on_a).await.unwrap();
    b.engine.confirm_peer_trusted(&a_id_on_b).await.unwrap();
    (b_id_on_a, a_id_on_b)
}

async fn expect_text(rx: &mut mpsc::Receiver<EngineEvent>, text: &str) {
    let want = text.to_string();
    wait_for(rx, 30, |e| matches!(e, EngineEvent::MessageReceived(m) if m.content == want)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs network (iroh relay)"]
async fn pair_chat_transfer_reconnect() {
    let tmp = tempfile::tempdir().unwrap();
    let (da, db) = (tmp.path().join("a"), tmp.path().join("b"));
    let mut a = spawn_peer(&da).await;
    let mut b = spawn_peer(&db).await;

    let (b_id, a_id) = pair(&mut a, &mut b).await;
    assert!(b_id.starts_with("peer:") && a_id.starts_with("peer:"));

    // Chat both ways (responder can reply once the initiator opened the ratchet).
    a.engine.send_message(&b_id, "hello from A").await.unwrap();
    expect_text(&mut b.events, "hello from A").await;
    b.engine.send_message(&a_id, "hello from B").await.unwrap();
    expect_text(&mut a.events, "hello from B").await;

    // File transfer A → B (multi-chunk, verified by content).
    let src = tmp.path().join("payload.bin");
    let data: Vec<u8> = (0..1_500_000u32).map(|i| (i * 31 % 251) as u8).collect();
    std::fs::write(&src, &data).unwrap();
    let (tid, _, _) = a.engine.send_file(&b_id, src.to_str().unwrap()).await.unwrap();
    let ev = wait_for(&mut b.events, 120, |e| {
        matches!(e, EngineEvent::TransferComplete { id, .. } if *id == tid)
            || matches!(e, EngineEvent::TransferFailed { .. })
    })
    .await;
    let EngineEvent::TransferComplete { path, .. } = ev else { panic!("transfer failed: {ev:?}") };
    assert_eq!(std::fs::read(&path).unwrap(), data, "received file content differs");
    assert!(std::path::Path::new(&path).starts_with(&db), "file must land in receive dir");

    // Reconnect from saved QR: auto-trusted, no new SAS needed, chat still works.
    let b_qr = b.engine.qr_payload_async().await.unwrap();
    a.engine.disconnect_peer(&b_id).await.unwrap();
    wait_for(&mut b.events, 30, |e| matches!(e, EngineEvent::PeerDisconnected { .. })).await;
    let (id2, _, auto) = a.engine.connect_and_verify(&b_qr).await.expect("reconnect");
    assert_eq!(id2, b_id);
    assert!(auto, "previously verified peer must be auto-trusted");
    a.engine.send_message(&b_id, "after reconnect").await.unwrap();
    expect_text(&mut b.events, "after reconnect").await;
    b.engine.send_message(&a_id, "reply after reconnect").await.unwrap();
    expect_text(&mut a.events, "reply after reconnect").await;

    a.engine.shutdown().await;
    b.engine.shutdown().await;
}

/// Both sides dial each other at the same moment (e.g. both apps start and auto-reconnect).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs network (iroh relay)"]
async fn simultaneous_reconnect_converges() {
    let tmp = tempfile::tempdir().unwrap();
    let mut a = spawn_peer(&tmp.path().join("a")).await;
    let mut b = spawn_peer(&tmp.path().join("b")).await;
    let (b_id, a_id) = pair(&mut a, &mut b).await;
    a.engine.send_message(&b_id, "warmup").await.unwrap();
    expect_text(&mut b.events, "warmup").await;

    let a_qr = a.engine.qr_payload_async().await.unwrap();
    let b_qr = b.engine.qr_payload_async().await.unwrap();
    a.engine.disconnect_peer(&b_id).await.unwrap();
    b.engine.disconnect_peer(&a_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (ea, eb) = (a.engine.clone(), b.engine.clone());
    let (ra, rb) = tokio::join!(
        async move { ea.connect_and_verify(&b_qr).await },
        async move { eb.connect_and_verify(&a_qr).await },
    );
    // At least one dial must win; the other may report it was superseded.
    eprintln!("dial results: A={ra:?} B={rb:?}");
    assert!(ra.is_ok() || rb.is_ok(), "both dials failed: {ra:?} / {rb:?}");

    // Whatever connection survived must carry traffic both ways.
    let mut ok = false;
    for _ in 0..20 {
        if a.engine.is_peer_trusted(&b_id).await && b.engine.is_peer_trusted(&a_id).await {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(ok, "peers did not converge on a trusted session");
    a.engine.send_message(&b_id, "glare A→B").await.unwrap();
    expect_text(&mut b.events, "glare A→B").await;
    b.engine.send_message(&a_id, "glare B→A").await.unwrap();
    expect_text(&mut a.events, "glare B→A").await;

    a.engine.shutdown().await;
    b.engine.shutdown().await;
}

/// Messages from a peer that has not passed SAS must not reach the UI until confirmed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs network (iroh relay)"]
async fn unverified_peer_messages_are_held() {
    let tmp = tempfile::tempdir().unwrap();
    let a = spawn_peer(&tmp.path().join("a")).await;
    let mut b = spawn_peer(&tmp.path().join("b")).await;
    let b_qr = b.engine.qr_payload_async().await.unwrap();
    let (b_id, _, _) = a.engine.connect_and_verify(&b_qr).await.unwrap();
    let ev = wait_for(&mut b.events, 30, |e| matches!(e, EngineEvent::SasReady { .. })).await;
    let EngineEvent::SasReady { peer_id: a_id, .. } = ev else { unreachable!() };

    // A trusts B and sends; B has not confirmed yet.
    a.engine.confirm_peer_trusted(&b_id).await.unwrap();
    a.engine.send_message(&b_id, "early").await.unwrap();
    let early = try_wait_for(&mut b.events, 3, |e| matches!(e, EngineEvent::MessageReceived(_))).await;
    assert!(early.is_none(), "message delivered before SAS confirmation");

    b.engine.confirm_peer_trusted(&a_id).await.unwrap();
    expect_text(&mut b.events, "early").await;

    a.engine.shutdown().await;
    b.engine.shutdown().await;
}
