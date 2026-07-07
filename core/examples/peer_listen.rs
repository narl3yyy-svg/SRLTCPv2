//! Listen for inbound QUIC peers (run on 10.0.30.101 or any host).

use std::time::Duration;

use srltcp_core::init_crypto;
use srltcp_core::init_logging;
use srltcp_core::p2p::{EngineEvent, P2pEngine};

#[tokio::main]
async fn main() {
    init_crypto();
    init_logging("info");

    let (engine, mut events) = P2pEngine::new();
    engine.start(9473).await.expect("start");
    println!("Listening on 0.0.0.0:9473");
    println!("QR: {}", engine.qr_payload());

    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    while tokio::time::Instant::now() < deadline {
        if let Some(event) = events.recv().await {
            match event {
                EngineEvent::PeerConnected { peer_id, .. } => {
                    println!("Peer connected: {peer_id}");
                }
                EngineEvent::MessageReceived(msg) => {
                    println!("Message from {}: {}", msg.sender_id, msg.content);
                }
                EngineEvent::Error(e) => eprintln!("Error: {e}"),
                other => println!("Event: {other:?}"),
            }
        }
    }
    engine.shutdown().await;
}