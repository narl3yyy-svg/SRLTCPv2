//! LAN test using this machine's IP (simulates desktop → peer on LAN).

use std::env;
use std::net::UdpSocket;
use std::time::Duration;

use srltcp_core::init_crypto;
use srltcp_core::init_logging;
use srltcp_core::p2p::{EngineEvent, P2pEngine};

fn local_lan_ip() -> String {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("udp bind");
    socket.connect("10.255.255.255:1").ok();
    let local = socket.local_addr().expect("local addr");
    local.ip().to_string()
}

#[tokio::main]
async fn main() {
    init_crypto();
    init_logging("info");

    let args: Vec<String> = env::args().collect();
    let target_ip = args.get(1).map(|s| s.as_str()).unwrap_or("10.0.30.101");
    let remote_qr = args.get(2).map(|s| s.as_str()).unwrap_or(
        "AjTqU9MmHMBy3dpi6xmxRTloSwOTD46pCpIN55kWHq3Z",
    );
    let message = args.get(3).cloned().unwrap_or_else(|| {
        format!("SRLTCPv2 LAN test {}", chrono::Utc::now().to_rfc3339())
    });

    let lan_ip = local_lan_ip();
    println!("Local LAN IP: {lan_ip}");

    // Peer simulator on 9473
    let (server, mut server_events) = P2pEngine::new();
    server.start(9473).await.expect("server start");
    let server_qr = server.qr_payload();
    println!("Peer simulator listening on {lan_ip}:9473 (QR: {server_qr})");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Client (desktop path) — connect to target
    let (client, mut client_events) = P2pEngine::new();
    client.start(9474).await.expect("client start");

    let addr = format!("{target_ip}:9473");
    let peer_id = format!("quic:{addr}");

    println!("Connecting to {addr}...");
    match client.connect_quic(&addr).await {
        Ok(()) => println!("Connected to {addr}"),
        Err(e) => {
            eprintln!("Connect failed: {e}");
            if target_ip == "10.0.30.101" && lan_ip != "10.0.30.101" {
                eprintln!("Remote peer not reachable; falling back to local peer at {lan_ip}:9473");
                let fallback = format!("{lan_ip}:9473");
                let peer_id = format!("quic:{fallback}");
                client
                    .connect_quic(&fallback)
                    .await
                    .expect("fallback connect");
                run_exchange(&client, &peer_id, &server_qr, &message, &mut server_events, &mut client_events)
                    .await;
            } else {
                std::process::exit(1);
            }
            client.shutdown().await;
            server.shutdown().await;
            return;
        }
    }

    run_exchange(
        &client,
        &peer_id,
        remote_qr,
        &message,
        &mut server_events,
        &mut client_events,
    )
    .await;

    client.shutdown().await;
    server.shutdown().await;
    println!("LAN test complete.");
}

async fn run_exchange(
    client: &P2pEngine,
    peer_id: &str,
    remote_qr: &str,
    message: &str,
    server_events: &mut tokio::sync::mpsc::Receiver<EngineEvent>,
    client_events: &mut tokio::sync::mpsc::Receiver<EngineEvent>,
) {
    client
        .handshake_with(peer_id, remote_qr)
        .await
        .expect("handshake");
    client
        .send_message(peer_id, message)
        .await
        .expect("send");
    println!("Message sent: {message}");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Some(event) = server_events.recv() => {
                if let EngineEvent::MessageReceived(msg) = event {
                    println!("Peer received: {}", msg.content);
                    return;
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
    eprintln!("No message received at peer within timeout");
    std::process::exit(1);
}