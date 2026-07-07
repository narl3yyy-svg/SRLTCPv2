//! SRLTCP v0.2.3 Desktop — Tauri v2 backend with graceful shutdown.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use srltcp_core::p2p::{EngineEvent, P2pEngine};
use tauri::{Emitter, Manager, State};
use tokio::sync::Mutex;

struct AppState {
    engine: Arc<Mutex<P2pEngine>>,
    shutting_down: Arc<AtomicBool>,
}

async fn graceful_shutdown(engine: Arc<Mutex<P2pEngine>>) {
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        engine.lock().await.shutdown().await;
    })
    .await;
}

#[tauri::command]
async fn get_public_key(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.engine.lock().await.public_key_hex())
}

#[tauri::command]
async fn get_qr_payload(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.engine.lock().await.qr_payload())
}

#[tauri::command]
async fn get_qr_image(state: State<'_, AppState>) -> Result<String, String> {
    let payload = state.engine.lock().await.qr_payload();
    srltcp_core::qr_png_data_url(&payload)
}

#[tauri::command]
async fn list_serial_ports() -> Result<Vec<String>, String> {
    Ok(P2pEngine::available_serial_ports())
}

#[tauri::command]
async fn connect_serial(
    state: State<'_, AppState>,
    port_name: String,
    baud_rate: u32,
) -> Result<(), String> {
    state
        .engine
        .lock()
        .await
        .connect_serial(&port_name, baud_rate)
        .await
}

#[tauri::command]
async fn connect_quic(state: State<'_, AppState>, addr: String) -> Result<String, String> {
    state.engine.lock().await.connect_quic(&addr).await?;
    Ok(format!("quic:{addr}"))
}

#[tauri::command]
async fn connect_and_verify(
    state: State<'_, AppState>,
    remote_qr: String,
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    let engine = state.engine.lock().await;
    let peer_id = if let Some(ref address) = addr {
        if !address.is_empty() {
            engine.connect_quic(address).await?;
            format!("quic:{address}")
        } else {
            return Err("address required for outbound connection".into());
        }
    } else {
        let peers = engine.connected_peers().await;
        peers
            .first()
            .cloned()
            .ok_or_else(|| {
                "No peer connected yet. Share your QR and wait for a peer, or use Advanced IP connect."
                    .to_string()
            })?
    };

    let sas = engine.handshake_with(&peer_id, &remote_qr).await?;
    Ok(serde_json::json!({ "peer_id": peer_id, "sas": sas }))
}

#[tauri::command]
async fn disconnect_peer(state: State<'_, AppState>, peer_id: String) -> Result<(), String> {
    state.engine.lock().await.disconnect_peer(&peer_id).await
}

#[tauri::command]
async fn handshake(
    state: State<'_, AppState>,
    peer_id: String,
    remote_qr: String,
) -> Result<String, String> {
    state
        .engine
        .lock()
        .await
        .handshake_with(&peer_id, &remote_qr)
        .await
}

#[tauri::command]
async fn send_message(
    state: State<'_, AppState>,
    peer_id: String,
    content: String,
) -> Result<(), String> {
    state
        .engine
        .lock()
        .await
        .send_message(&peer_id, &content)
        .await
}

#[tauri::command]
async fn send_file(
    state: State<'_, AppState>,
    peer_id: String,
    file_path: String,
) -> Result<serde_json::Value, String> {
    let (transfer_id, filename, progress) = state
        .engine
        .lock()
        .await
        .send_file(&peer_id, &file_path)
        .await?;
    Ok(serde_json::json!({
        "transfer_id": transfer_id,
        "filename": filename,
        "progress": progress,
    }))
}

#[tauri::command]
async fn get_peers(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.engine.lock().await.connected_peers().await)
}

#[tauri::command]
async fn start_voice_call(state: State<'_, AppState>, peer_id: String) -> Result<String, String> {
    state.engine.lock().await.start_call(&peer_id, false).await
}

#[tauri::command]
async fn start_video_call(state: State<'_, AppState>, peer_id: String) -> Result<String, String> {
    state.engine.lock().await.start_call(&peer_id, true).await
}

#[tauri::command]
async fn end_call(state: State<'_, AppState>, call_id: String) -> Result<(), String> {
    state.engine.lock().await.end_call(&call_id).await
}

#[tauri::command]
async fn shutdown_engine(state: State<'_, AppState>) -> Result<(), String> {
    graceful_shutdown(state.engine.clone()).await;
    Ok(())
}

fn install_shutdown_handler(engine: Arc<Mutex<P2pEngine>>, shutting_down: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        use signal_hook::consts::signal::{SIGINT, SIGTERM};
        use signal_hook::iterator::Signals;

        let mut signals = Signals::new([SIGTERM, SIGINT]).expect("signal handler");
        for _ in signals.forever() {
            if shutting_down.swap(true, Ordering::SeqCst) {
                std::process::exit(0);
            }
            tracing::info!("shutdown signal received — releasing resources");
            tauri::async_runtime::block_on(graceful_shutdown(engine.clone()));
            std::process::exit(0);
        }
    });
}

async fn run_auto_peer_test(engine: Arc<Mutex<P2pEngine>>) -> Result<(), String> {
    let addr = std::env::var("SRLTCP_TEST_ADDR").unwrap_or_else(|_| "10.0.30.101:9473".into());
    let remote_qr = std::env::var("SRLTCP_TEST_QR")
        .unwrap_or_else(|_| "AjTqU9MmHMBy3dpi6xmxRTloSwOTD46pCpIN55kWHq3Z".into());
    let message = std::env::var("SRLTCP_TEST_MSG")
        .unwrap_or_else(|_| "SRLTCPv2-0.2.3 desktop auto-test message".into());

    let client_port: u16 = std::env::var("SRLTCP_CLIENT_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9474);

    let peer_id = format!("quic:{addr}");
    {
        let e = engine.lock().await;
        e.start(client_port).await?;
        e.connect_quic(&addr).await?;
        e.handshake_with(&peer_id, &remote_qr).await?;
        e.send_message(&peer_id, &message).await?;
        tracing::info!(%addr, %message, "desktop auto-test message sent");
    }
    graceful_shutdown(engine).await;
    Ok(())
}

fn main() {
    srltcp_core::init_crypto();
    srltcp_core::init_logging("info");

    if std::env::var("SRLTCP_AUTO_TEST").is_ok() {
        let (engine, _) = P2pEngine::new();
        let engine = Arc::new(Mutex::new(engine));
        let result = tauri::async_runtime::block_on(run_auto_peer_test(engine));
        match result {
            Ok(()) => {
                println!("Desktop peer test: message sent successfully");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Desktop peer test failed: {e}");
                std::process::exit(1);
            }
        }
    }

    let (engine, mut event_rx) = P2pEngine::new();
    let engine = Arc::new(Mutex::new(engine));
    let shutting_down = Arc::new(AtomicBool::new(false));

    install_shutdown_handler(engine.clone(), shutting_down.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_public_key,
            get_qr_payload,
            get_qr_image,
            list_serial_ports,
            connect_serial,
            connect_quic,
            connect_and_verify,
            disconnect_peer,
            handshake,
            send_message,
            send_file,
            get_peers,
            start_voice_call,
            start_video_call,
            end_call,
            shutdown_engine,
        ])
        .manage(AppState {
            engine: engine.clone(),
            shutting_down: shutting_down.clone(),
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let eng = engine.clone();

            tauri::async_runtime::spawn(async move {
                let e = eng.lock().await;
                if let Err(err) = e.start(9473).await {
                    tracing::error!(error = %err, "failed to start engine");
                }
            });

            tauri::async_runtime::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    let payload = match event {
                        EngineEvent::MessageReceived(msg) => serde_json::json!({
                            "type": "message",
                            "id": msg.id.to_string(),
                            "sender": msg.sender_id,
                            "content": msg.content,
                            "timestamp": msg.timestamp.to_rfc3339(),
                        }),
                        EngineEvent::PeerConnected { peer_id, transport } => {
                            serde_json::json!({
                                "type": "peer_connected",
                                "peer_id": peer_id,
                                "transport": format!("{transport:?}"),
                            })
                        }
                        EngineEvent::PeerDisconnected { peer_id, reason } => {
                            serde_json::json!({
                                "type": "peer_disconnected",
                                "peer_id": peer_id,
                                "reason": reason,
                            })
                        }
                        EngineEvent::SasReady { peer_id, sas } => serde_json::json!({
                            "type": "sas_ready",
                            "peer_id": peer_id,
                            "sas": sas,
                        }),
                        EngineEvent::TransferProgress { id, filename, progress } => {
                            serde_json::json!({
                                "type": "transfer_progress",
                                "id": id,
                                "filename": filename,
                                "progress": progress,
                            })
                        }
                        EngineEvent::TransferComplete { id, filename } => {
                            serde_json::json!({
                                "type": "transfer_complete",
                                "id": id,
                                "filename": filename,
                            })
                        }
                        EngineEvent::CallStarted { call_id, peer_id, is_video } => {
                            serde_json::json!({
                                "type": "call_started",
                                "call_id": call_id,
                                "peer_id": peer_id,
                                "is_video": is_video,
                            })
                        }
                        EngineEvent::CallEnded { call_id } => {
                            serde_json::json!({ "type": "call_ended", "call_id": call_id })
                        }
                        EngineEvent::Started => serde_json::json!({ "type": "started" }),
                        EngineEvent::Stopped => serde_json::json!({ "type": "stopped" }),
                        EngineEvent::Error(e) => {
                            serde_json::json!({ "type": "error", "message": e })
                        }
                    };
                    let _ = handle.emit("srltcp-event", payload);
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window
                    .state::<AppState>()
                    .shutting_down
                    .swap(true, Ordering::SeqCst)
                {
                    return;
                }
                api.prevent_close();
                let state = window.state::<AppState>();
                let eng = state.engine.clone();
                let app_handle = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    graceful_shutdown(eng).await;
                    app_handle.exit(0);
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error running SRLTCP desktop");
}