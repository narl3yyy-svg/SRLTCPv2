//! SRLTCP v0.2.0 — Secure Reliable LAN/TCP/Serial P2P messaging core.

pub mod crypto;
pub mod qr_image;
pub mod network;
pub mod p2p;
pub mod protocol;
pub mod serial;
pub mod transfer;
pub mod webrtc;

pub use crypto::{
    compute_sas, parse_qr_payload, HybridKeyExchange, Identity, ParsedQr, SessionRatchet,
};
pub use qr_image::qr_png_data_url;
pub use network::{IrohTransport, TransportKind};
pub use p2p::{EngineEvent, P2pEngine};
pub use protocol::{ChatMessage, Envelope, MessageType};
pub use serial::{list_ports, Frame, ReliabilityLayer, SerialConfig, SerialTransport};
pub use transfer::{ChunkedReceiver, ChunkedSender, TransferManifest};
pub use webrtc::{CallSession, CallState, WebRtcError};

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::runtime::Runtime;
use tokio::sync::Mutex;

/// Library version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_QUIC_PORT: u16 = 9473;

/// Initialize crypto subsystem. iroh manages transport TLS internally.
pub fn init_crypto() {}

/// Initialize tracing/logging. Call once at startup.
pub fn init_logging(level: &str) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

// ── UniFFI exports (must be in scope for scaffolding) ──────────────

/// UniFFI-compatible event for foreign language bindings.
#[derive(Debug, Clone)]
pub struct SrltcpEvent {
    pub event_type: String,
    pub peer_id: Option<String>,
    pub message: Option<String>,
    pub content: Option<String>,
    pub sas: Option<String>,
    pub transfer_id: Option<String>,
    pub filename: Option<String>,
    pub progress: Option<f64>,
    pub transport: Option<String>,
    pub call_id: Option<String>,
    pub error: Option<String>,
    pub auto_trusted: Option<bool>,
}

/// Transfer metadata returned to Kotlin/Swift.
#[derive(Debug, Clone)]
pub struct TransferInfo {
    pub transfer_id: String,
    pub filename: String,
    pub total_size: u64,
    pub progress: f64,
}

/// Result of QR connect + SAS handshake.
#[derive(Debug, Clone)]
pub struct ConnectResult {
    pub peer_id: String,
    pub sas: String,
    pub auto_trusted: bool,
}

fn engine_event_to_uniffi(event: EngineEvent) -> SrltcpEvent {
    match event {
        EngineEvent::Started => SrltcpEvent {
            event_type: "started".into(),
            peer_id: None,
            message: None,
            content: None,
            sas: None,
            transfer_id: None,
            filename: None,
            progress: None,
            transport: None,
            call_id: None,
            error: None,
            auto_trusted: None,
        },
        EngineEvent::Stopped => SrltcpEvent {
            event_type: "stopped".into(),
            peer_id: None,
            message: None,
            content: None,
            sas: None,
            transfer_id: None,
            filename: None,
            progress: None,
            transport: None,
            call_id: None,
            error: None,
            auto_trusted: None,
        },
        EngineEvent::PeerConnected { peer_id, transport } => SrltcpEvent {
            event_type: "peer_connected".into(),
            peer_id: Some(peer_id),
            message: None,
            content: None,
            sas: None,
            transfer_id: None,
            filename: None,
            progress: None,
            transport: Some(format!("{transport:?}")),
            call_id: None,
            error: None,
            auto_trusted: None,
        },
        EngineEvent::PeerDisconnected { peer_id, reason } => SrltcpEvent {
            event_type: "peer_disconnected".into(),
            peer_id: Some(peer_id),
            message: Some(reason),
            content: None,
            sas: None,
            transfer_id: None,
            filename: None,
            progress: None,
            transport: None,
            call_id: None,
            error: None,
            auto_trusted: None,
        },
        EngineEvent::MessageReceived(msg) => SrltcpEvent {
            event_type: "message".into(),
            peer_id: Some(msg.sender_id),
            message: None,
            content: Some(msg.content),
            sas: None,
            transfer_id: None,
            filename: None,
            progress: None,
            transport: None,
            call_id: None,
            error: None,
            auto_trusted: None,
        },
        EngineEvent::SasReady {
            peer_id,
            sas,
            auto_trusted,
        } => SrltcpEvent {
            event_type: "sas_ready".into(),
            peer_id: Some(peer_id),
            message: None,
            content: None,
            sas: Some(sas),
            transfer_id: None,
            filename: None,
            progress: None,
            transport: None,
            call_id: None,
            error: None,
            auto_trusted: Some(auto_trusted),
        },
        EngineEvent::PeerIdUpdated { old_id, new_id } => SrltcpEvent {
            event_type: "peer_id_updated".into(),
            peer_id: Some(new_id),
            message: Some(old_id),
            content: None,
            sas: None,
            transfer_id: None,
            filename: None,
            progress: None,
            transport: None,
            call_id: None,
            error: None,
            auto_trusted: None,
        },
        EngineEvent::TransferProgress { id, filename, progress } => SrltcpEvent {
            event_type: "transfer_progress".into(),
            peer_id: None,
            message: None,
            content: None,
            sas: None,
            transfer_id: Some(id),
            filename: Some(filename),
            progress: Some(progress),
            transport: None,
            call_id: None,
            error: None,
            auto_trusted: None,
        },
        EngineEvent::TransferComplete { id, filename } => SrltcpEvent {
            event_type: "transfer_complete".into(),
            peer_id: None,
            message: None,
            content: None,
            sas: None,
            transfer_id: Some(id),
            filename: Some(filename),
            progress: Some(1.0),
            transport: None,
            call_id: None,
            error: None,
            auto_trusted: None,
        },
        EngineEvent::CallStarted { call_id, peer_id, is_video } => SrltcpEvent {
            event_type: if is_video {
                "video_call_started".into()
            } else {
                "voice_call_started".into()
            },
            peer_id: Some(peer_id),
            message: None,
            content: None,
            sas: None,
            transfer_id: None,
            filename: None,
            progress: None,
            transport: None,
            call_id: Some(call_id),
            error: None,
            auto_trusted: None,
        },
        EngineEvent::CallEnded { call_id } => SrltcpEvent {
            event_type: "call_ended".into(),
            peer_id: None,
            message: None,
            content: None,
            sas: None,
            transfer_id: None,
            filename: None,
            progress: None,
            transport: None,
            call_id: Some(call_id),
            error: None,
            auto_trusted: None,
        },
        EngineEvent::Error(e) => SrltcpEvent {
            event_type: "error".into(),
            peer_id: None,
            message: None,
            content: None,
            sas: None,
            transfer_id: None,
            filename: None,
            progress: None,
            transport: None,
            call_id: None,
            error: Some(e),
            auto_trusted: None,
        },
    }
}

/// Returns the library version.
pub fn version() -> String {
    VERSION.to_string()
}

// Scaffolding must come before callback trait usage
uniffi::include_scaffolding!("srltcp_core");

/// UniFFI-exported P2P engine wrapper.
pub struct SrltcpEngine {
    inner: Arc<Mutex<P2pEngine>>,
    runtime: Runtime,
    events: Arc<StdMutex<VecDeque<SrltcpEvent>>>,
}

impl SrltcpEngine {
    pub fn new() -> Self {
        init_crypto();
        let runtime = Runtime::new().expect("tokio runtime");
        let (engine, mut event_rx) = P2pEngine::new();
        let events: Arc<StdMutex<VecDeque<SrltcpEvent>>> =
            Arc::new(StdMutex::new(VecDeque::with_capacity(64)));
        let events_for_task = events.clone();

        runtime.spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let uni_event = engine_event_to_uniffi(event);
                if let Ok(mut q) = events_for_task.lock() {
                    q.push_back(uni_event);
                }
            }
        });

        Self {
            inner: Arc::new(Mutex::new(engine)),
            runtime,
            events,
        }
    }

    pub fn poll_event(&self) -> Option<SrltcpEvent> {
        self.events.lock().ok()?.pop_front()
    }

    pub fn drain_events(&self) -> Vec<SrltcpEvent> {
        let mut out = Vec::new();
        if let Ok(mut q) = self.events.lock() {
            out.extend(q.drain(..));
        }
        out
    }

    pub fn public_key_hex(&self) -> String {
        self.runtime.block_on(async {
            self.inner.lock().await.public_key_hex()
        })
    }

    pub fn qr_payload(&self) -> String {
        self.runtime.block_on(async {
            match self.inner.lock().await.qr_payload_async().await {
                Ok(payload) => payload,
                Err(_) => self.inner.lock().await.qr_payload(),
            }
        })
    }

    pub fn qr_image_data_url(&self) -> String {
        let payload = self.qr_payload();
        qr_png_data_url(&payload).unwrap_or_default()
    }

    pub fn confirm_peer_trusted(&self, peer_id: String) {
        let inner = self.inner.clone();
        self.runtime.block_on(async move {
            if let Err(e) = inner.lock().await.confirm_peer_trusted(&peer_id).await {
                tracing::error!(error = %e, "confirm_peer_trusted failed");
            }
        });
    }

    pub fn is_peer_trusted(&self, peer_id: String) -> bool {
        self.runtime.block_on(async {
            self.inner.lock().await.is_peer_trusted(&peer_id).await
        })
    }

    pub fn is_peer_connected(&self, peer_id: String) -> bool {
        self.runtime.block_on(async {
            self.inner.lock().await.is_peer_connected(&peer_id).await
        })
    }

    pub fn iroh_ticket(&self) -> Option<String> {
        self.runtime.block_on(async {
            self.inner.lock().await.iroh_ticket().await.ok()
        })
    }

    /// Deprecated — use `iroh_ticket()`.
    pub fn local_endpoint(&self) -> Option<String> {
        self.iroh_ticket()
    }

    pub fn connect_and_verify(&self, remote_qr: String) -> ConnectResult {
        self.runtime.block_on(async {
            match self
                .inner
                .lock()
                .await
                .connect_and_verify(&remote_qr)
                .await
            {
                Ok((peer_id, sas, auto_trusted)) => ConnectResult {
                    peer_id,
                    sas,
                    auto_trusted,
                },
                Err(e) => ConnectResult {
                    peer_id: String::new(),
                    sas: format!("error: {e}"),
                    auto_trusted: false,
                },
            }
        })
    }

    pub fn available_serial_ports(&self) -> Vec<String> {
        P2pEngine::available_serial_ports()
            .into_iter()
            .map(|e| e.label)
            .collect()
    }

    pub fn start(&self, quic_port: u16) {
        let inner = self.inner.clone();
        self.runtime.spawn(async move {
            if let Err(e) = inner.lock().await.start(quic_port).await {
                tracing::error!(error = %e, "engine start failed");
            }
        });
    }

    pub fn connect_serial(&self, port_name: String, baud_rate: u32) {
        let inner = self.inner.clone();
        self.runtime.spawn(async move {
            if let Err(e) = inner.lock().await.connect_serial(&port_name, baud_rate).await {
                tracing::error!(error = %e, "serial connect failed");
            }
        });
    }

    pub fn connect_quic(&self, addr: String) {
        let inner = self.inner.clone();
        self.runtime.spawn(async move {
            if let Err(e) = inner.lock().await.connect_quic(&addr).await {
                tracing::error!(error = %e, "quic connect failed");
            }
        });
    }

    pub fn load_trusted_pubkeys(&self, pubkeys: Vec<String>) {
        let inner = self.inner.clone();
        self.runtime.block_on(async move {
            inner.lock().await.load_trusted_pubkeys(pubkeys).await;
        });
    }

    pub fn set_receive_dir(&self, path: String) {
        let inner = self.inner.clone();
        self.runtime.block_on(async move {
            inner
                .lock()
                .await
                .set_receive_dir(PathBuf::from(path))
                .await;
        });
    }

    pub fn disconnect_peer(&self, peer_id: String) {
        let inner = self.inner.clone();
        self.runtime.spawn(async move {
            if let Err(e) = inner.lock().await.disconnect_peer(&peer_id).await {
                tracing::error!(error = %e, "disconnect failed");
            }
        });
    }

    pub fn handshake_with(&self, peer_id: String, remote_qr: String) -> String {
        self.runtime.block_on(async {
            match self
                .inner
                .lock()
                .await
                .handshake_with(&peer_id, &remote_qr)
                .await
            {
                Ok(sas) => sas,
                Err(e) => format!("error: {e}"),
            }
        })
    }

    pub fn send_message(&self, peer_id: String, content: String) {
        let inner = self.inner.clone();
        self.runtime.spawn(async move {
            if let Err(e) = inner.lock().await.send_message(&peer_id, &content).await {
                tracing::error!(error = %e, "send failed");
            }
        });
    }

    pub fn send_file(&self, peer_id: String, file_path: String) -> TransferInfo {
        self.runtime.block_on(async {
            match self.inner.lock().await.send_file(&peer_id, &file_path).await {
                Ok((transfer_id, filename, progress)) => TransferInfo {
                    transfer_id,
                    filename: filename.clone(),
                    total_size: 0,
                    progress,
                },
                Err(e) => {
                    tracing::error!(error = %e, "file transfer failed");
                    TransferInfo {
                        transfer_id: String::new(),
                        filename: format!("error: {e}"),
                        total_size: 0,
                        progress: 0.0,
                    }
                }
            }
        })
    }

    pub fn start_voice_call(&self, peer_id: String) -> String {
        self.runtime.block_on(async {
            self.inner
                .lock()
                .await
                .start_call(&peer_id, false)
                .await
                .unwrap_or_else(|e| format!("error: {e}"))
        })
    }

    pub fn start_video_call(&self, peer_id: String) -> String {
        self.runtime.block_on(async {
            self.inner
                .lock()
                .await
                .start_call(&peer_id, true)
                .await
                .unwrap_or_else(|e| format!("error: {e}"))
        })
    }

    pub fn end_call(&self, call_id: String) {
        let inner = self.inner.clone();
        self.runtime.spawn(async move {
            if let Err(e) = inner.lock().await.end_call(&call_id).await {
                tracing::error!(error = %e, "end call failed");
            }
        });
    }

    pub fn shutdown(&self) {
        let inner = self.inner.clone();
        self.runtime.block_on(async {
            inner.lock().await.shutdown().await;
        });
    }

    pub fn is_running(&self) -> bool {
        self.runtime.block_on(async {
            self.inner.lock().await.is_running().await
        })
    }

    pub fn connected_peers(&self) -> Vec<String> {
        self.runtime.block_on(async {
            self.inner.lock().await.connected_peers().await
        })
    }

    pub fn default_quic_port(&self) -> u16 {
        DEFAULT_QUIC_PORT
    }
}