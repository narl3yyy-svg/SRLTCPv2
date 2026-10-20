//! Serial transport end-to-end over a virtual null-modem cable (socat pty pair).
//!
//! Run with: cargo test -p srltcp-core --test serial_e2e -- --ignored

use std::process::{Child, Command};
use std::time::Duration;

use srltcp_core::p2p::{EngineEvent, P2pEngine};
use tokio::sync::mpsc;

struct Cable {
    child: Child,
    a: String,
    b: String,
}

impl Drop for Cable {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn virtual_cable(dir: &std::path::Path) -> Cable {
    let a = dir.join("ttyA").to_string_lossy().to_string();
    let b = dir.join("ttyB").to_string_lossy().to_string();
    let child = Command::new("socat")
        .arg(format!("pty,raw,echo=0,link={a}"))
        .arg(format!("pty,raw,echo=0,link={b}"))
        .spawn()
        .expect("socat must be installed for this test");
    for _ in 0..50 {
        if std::path::Path::new(&a).exists() && std::path::Path::new(&b).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Cable { child, a, b }
}

async fn wait_for<F>(rx: &mut mpsc::Receiver<EngineEvent>, secs: u64, mut pred: F) -> EngineEvent
where
    F: FnMut(&EngineEvent) -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let ev = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("timed out waiting for engine event")
            .expect("event channel closed");
        if pred(&ev) {
            return ev;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs socat"]
async fn serial_pair_chat_and_file() {
    let tmp = tempfile::tempdir().unwrap();
    let cable = virtual_cable(tmp.path());
    let (a, mut a_rx) = P2pEngine::new();
    let (b, mut b_rx) = P2pEngine::new();
    a.set_receive_dir(tmp.path().join("recv_a")).await;
    b.set_receive_dir(tmp.path().join("recv_b")).await;

    // A only opens its end; B opens the other end and verifies against A's QR.
    a.connect_serial(&cable.a, 115200).await.expect("open A");
    let a_qr = a.qr_payload();
    let (a_id, sas_b, auto) = b
        .connect_serial_and_verify(&cable.b, 115200, &a_qr)
        .await
        .expect("serial handshake");
    assert!(!auto);
    let ev = wait_for(&mut a_rx, 20, |e| matches!(e, EngineEvent::SasReady { .. })).await;
    let EngineEvent::SasReady { peer_id: b_id, sas: sas_a, .. } = ev else { unreachable!() };
    assert_eq!(sas_a, sas_b);
    assert_eq!(a_id, format!("peer:{}", a.public_key_hex()));

    b.confirm_peer_trusted(&a_id).await.unwrap();
    a.confirm_peer_trusted(&b_id).await.unwrap();

    b.send_message(&a_id, "hello over serial").await.unwrap();
    wait_for(&mut a_rx, 20, |e| matches!(e, EngineEvent::MessageReceived(m) if m.content == "hello over serial")).await;
    a.send_message(&b_id, "reply over serial").await.unwrap();
    wait_for(&mut b_rx, 20, |e| matches!(e, EngineEvent::MessageReceived(m) if m.content == "reply over serial")).await;

    // A message larger than one serial frame is fragmented and reassembled.
    let long = "x".repeat(20_000);
    a.send_message(&b_id, &long).await.unwrap();
    wait_for(&mut b_rx, 30, |e| matches!(e, EngineEvent::MessageReceived(m) if m.content.len() == 20_000)).await;

    let src = tmp.path().join("file.bin");
    let data: Vec<u8> = (0..60_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(&src, &data).unwrap();
    let (tid, _, _) = b.send_file(&a_id, src.to_str().unwrap()).await.unwrap();
    let ev = wait_for(&mut a_rx, 120, |e| {
        matches!(e, EngineEvent::TransferComplete { id, .. } if *id == tid) || matches!(e, EngineEvent::TransferFailed { .. })
    })
    .await;
    let EngineEvent::TransferComplete { path, .. } = ev else { panic!("transfer failed: {ev:?}") };
    assert_eq!(std::fs::read(path).unwrap(), data);

    // Disconnect releases the port so it can be opened again.
    b.disconnect_peer(&a_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    b.connect_serial(&cable.b, 115200).await.expect("port reopens after disconnect");

    a.shutdown().await;
    b.shutdown().await;
}
