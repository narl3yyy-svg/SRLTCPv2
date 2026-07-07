//! CLI peer connectivity test — same engine path as desktop/Android.
//!
//! Usage:
//!   cargo run --example peer_test -- 10.0.30.101:9473 <remote_qr> [message]

use std::env;
use std::time::Duration;

use srltcp_core::init_crypto;
use srltcp_core::init_logging;
use srltcp_core::p2p::{EngineEvent, P2pEngine};

#[tokio::main]
async fn main() {
    init_crypto();
    init_logging("info");

    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <addr> <remote_qr> [message]", args[0]);
        std::process::exit(1);
    }

    let addr = &args[1];
    let remote_qr = &args[2];
    let message = args.get(3).cloned().unwrap_or_else(|| {
        format!("SRLTCPv2 test message at {}", chrono::Utc::now().to_rfc3339())
    });

    let client_port: u16 = env::var("SRLTCP_CLIENT_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9474);

    let (engine, mut events) = P2pEngine::new();
    engine.start(client_port).await.expect("start engine");

    let peer_id = format!("quic:{addr}");
    engine.connect_quic(addr).await.expect("connect quic");
    engine
        .handshake_with(&peer_id, remote_qr)
        .await
        .expect("handshake");

    engine
        .send_message(&peer_id, &message)
        .await
        .expect("send message");

    println!("Message sent to {peer_id}: {message}");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Some(event) = events.recv() => {
                match event {
                    EngineEvent::MessageReceived(msg) => {
                        println!("Received reply: {}", msg.content);
                    }
                    EngineEvent::Error(e) => eprintln!("Engine error: {e}"),
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    engine.shutdown().await;
    println!("Test complete.");
}