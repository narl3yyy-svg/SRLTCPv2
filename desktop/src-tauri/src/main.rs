//! SRLTCP v0.2.16 Desktop — Tauri v2 backend with graceful shutdown.

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
    let engine = state.engine.lock().await;
    engine.wait_until_ready(30).await?;
    engine.qr_payload_async().await
}

#[tauri::command]
async fn get_qr_image(state: State<'_, AppState>) -> Result<String, String> {
    let payload = state.engine.lock().await.qr_payload_async().await?;
    srltcp_core::qr_png_data_url(&payload)
}

#[tauri::command]
async fn list_serial_ports() -> Result<Vec<srltcp_core::serial::SerialPortEntry>, String> {
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
async fn get_iroh_ticket(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(state.engine.lock().await.iroh_ticket().await.ok())
}

#[tauri::command]
async fn confirm_peer_trusted(state: State<'_, AppState>, peer_id: String) -> Result<(), String> {
    state
        .engine
        .lock()
        .await
        .confirm_peer_trusted(&peer_id)
        .await
}

#[tauri::command]
async fn connect_and_verify(
    state: State<'_, AppState>,
    remote_qr: String,
) -> Result<serde_json::Value, String> {
    let engine = state.engine.lock().await;
    engine.wait_until_ready(30).await?;
    let (peer_id, sas, auto_trusted) = engine.connect_and_verify(&remote_qr).await?;
    Ok(serde_json::json!({
        "peer_id": peer_id,
        "sas": sas,
        "auto_trusted": auto_trusted,
    }))
}

#[tauri::command]
async fn engine_is_ready(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.engine.lock().await.is_ready().await)
}

#[tauri::command]
async fn wait_for_engine(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.lock().await.wait_until_ready(30).await
}

#[tauri::command]
async fn load_trusted_pubkeys(
    state: State<'_, AppState>,
    pubkeys: Vec<String>,
) -> Result<(), String> {
    state.engine.lock().await.load_trusted_pubkeys(pubkeys).await;
    Ok(())
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
async fn send_call_signal(
    state: State<'_, AppState>,
    peer_id: String,
    call_id: String,
    signal: String,
    payload: String,
    is_video: bool,
) -> Result<(), String> {
    state
        .engine
        .lock()
        .await
        .send_call_signal(&peer_id, &call_id, &signal, &payload, is_video)
        .await
}

#[tauri::command]
async fn end_call(
    state: State<'_, AppState>,
    peer_id: String,
    call_id: String,
) -> Result<(), String> {
    state.engine.lock().await.end_call(&peer_id, &call_id).await
}

#[tauri::command]
async fn cancel_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<(), String> {
    state.engine.lock().await.cancel_transfer(&transfer_id).await
}

#[tauri::command]
async fn register_saved_peer(
    state: State<'_, AppState>,
    peer_id: String,
    qr: String,
) -> Result<(), String> {
    state.engine.lock().await.register_saved_peer(&peer_id, &qr).await;
    Ok(())
}

#[tauri::command]
async fn get_receive_dir(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state
        .engine
        .lock()
        .await
        .receive_dir()
        .await
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
async fn shutdown_engine(state: State<'_, AppState>) -> Result<(), String> {
    graceful_shutdown(state.engine.clone()).await;
    Ok(())
}

fn install_shutdown_handler(engine: Arc<Mutex<P2pEngine>>, shutting_down: Arc<AtomicBool>) {
    #[cfg(unix)]
    {
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
    #[cfg(not(unix))]
    {
        let _ = (engine, shutting_down);
    }
}

fn main() {
    srltcp_core::init_crypto();
    srltcp_core::init_logging("info");

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
            get_iroh_ticket,
            engine_is_ready,
            wait_for_engine,
            list_serial_ports,
            connect_serial,
            connect_and_verify,
            confirm_peer_trusted,
            load_trusted_pubkeys,
            disconnect_peer,
            handshake,
            send_message,
            send_file,
            get_peers,
            start_voice_call,
            start_video_call,
            send_call_signal,
            end_call,
            cancel_transfer,
            register_saved_peer,
            get_receive_dir,
            shutdown_engine,
        ])
        .manage(AppState {
            engine: engine.clone(),
            shutting_down: shutting_down.clone(),
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let eng = engine.clone();

            let recv_dir = app.path().app_data_dir().ok().map(|d| d.join("received"));
            tauri::async_runtime::spawn(async move {
                let eng = eng.clone();
                if let Some(recv) = recv_dir {
                    let _ = std::fs::create_dir_all(&recv);
                    eng.lock().await.set_receive_dir(recv).await;
                }
                let start_err = {
                    let e = eng.lock().await;
                    e.start(9473).await.err()
                };
                if let Some(err) = start_err {
                    tracing::error!(error = %err, "failed to start engine");
                    return;
                }
                let ready_err = {
                    let e = eng.lock().await;
                    e.wait_until_ready(30).await.err()
                };
                if let Some(err) = ready_err {
                    tracing::error!(error = %err, "iroh not ready after start");
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
                        EngineEvent::SasReady {
                            peer_id,
                            sas,
                            auto_trusted,
                        } => serde_json::json!({
                            "type": "sas_ready",
                            "peer_id": peer_id,
                            "sas": sas,
                            "auto_trusted": auto_trusted,
                        }),
                        EngineEvent::PeerIdUpdated { old_id, new_id } => {
                            serde_json::json!({
                                "type": "peer_id_updated",
                                "old_id": old_id,
                                "new_id": new_id,
                            })
                        }
                        EngineEvent::TransferProgress {
                            id,
                            filename,
                            progress,
                            peer_id,
                        } => {
                            serde_json::json!({
                                "type": "transfer_progress",
                                "id": id,
                                "filename": filename,
                                "progress": progress,
                                "peer_id": peer_id,
                            })
                        }
                        EngineEvent::TransferComplete {
                            id,
                            filename,
                            peer_id,
                            path,
                        } => {
                            serde_json::json!({
                                "type": "transfer_complete",
                                "id": id,
                                "filename": filename,
                                "peer_id": peer_id,
                                "path": path,
                            })
                        }
                        EngineEvent::TransferCancelled {
                            id,
                            filename,
                            peer_id,
                        } => {
                            serde_json::json!({
                                "type": "transfer_cancelled",
                                "id": id,
                                "filename": filename,
                                "peer_id": peer_id,
                            })
                        }
                        EngineEvent::CallSignaling {
                            call_id,
                            peer_id,
                            signal,
                            payload,
                            is_video,
                        } => {
                            serde_json::json!({
                                "type": format!("call_{signal}"),
                                "call_id": call_id,
                                "peer_id": peer_id,
                                "payload": payload,
                                "is_video": is_video,
                            })
                        }
                        EngineEvent::CallEnded { call_id } => {
                            serde_json::json!({ "type": "call_ended", "call_id": call_id })
                        }
                        EngineEvent::MessageQueued { peer_id, queue_size } => {
                            serde_json::json!({
                                "type": "message_queued",
                                "peer_id": peer_id,
                                "queue_size": queue_size,
                            })
                        }
                        EngineEvent::Reconnecting { peer_id } => {
                            serde_json::json!({
                                "type": "reconnecting",
                                "peer_id": peer_id,
                            })
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