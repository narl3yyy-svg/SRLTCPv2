//! Local QUIC loopback test — verifies send/receive without external peer.

use std::time::Duration;

use srltcp_core::init_crypto;
use srltcp_core::init_logging;
use srltcp_core::p2p::{EngineEvent, P2pEngine};

#[tokio::main]
async fn main() {
    init_crypto();
    init_logging("info");

    let (server, mut server_events) = P2pEngine::new();
    let (client, mut client_events) = P2pEngine::new();

    server.start(19473).await.expect("server start");
    client.start(19474).await.expect("client start");

    let client_qr = client.qr_payload();
    let addr = "127.0.0.1:19473";
    let peer_id = format!("quic:{addr}");

    client.connect_quic(addr).await.expect("connect");
    client
        .handshake_with(&peer_id, &client_qr)
        .await
        .expect("handshake");

    client
        .send_message(&peer_id, "loopback hello")
        .await
        .expect("send");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut got_message = false;

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Some(event) = server_events.recv() => {
                if let EngineEvent::MessageReceived(msg) = event {
                    println!("Server received: {}", msg.content);
                    got_message = true;
                    break;
                }
            }
            Some(event) = client_events.recv() => {
                if let EngineEvent::MessageReceived(msg) = event {
                    println!("Client received: {}", msg.content);
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }

    client.shutdown().await;
    server.shutdown().await;

    if got_message {
        println!("QUIC loopback OK");
    } else {
        eprintln!("QUIC loopback FAILED — no message received");
        std::process::exit(1);
    }
}