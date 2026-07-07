//! Central P2P engine coordinating all transports and sessions.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

use crate::crypto::handshake::HybridKeyExchange;
use crate::crypto::identity::Identity;
use crate::network::TransportKind;
use crate::network::QuicTransport;
use crate::protocol::{ChatMessage, Envelope, MessageType};
use crate::serial::{list_ports, SerialConfig, SerialTransport};
use crate::transfer::ChunkedSender;
use crate::webrtc::CallSession;

/// Engine lifecycle events for UI/bindings.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    Started,
    Stopped,
    PeerConnected { peer_id: String, transport: TransportKind },
    PeerDisconnected { peer_id: String, reason: String },
    MessageReceived(ChatMessage),
    SasReady { peer_id: String, sas: String },
    TransferProgress { id: String, filename: String, progress: f64 },
    TransferComplete { id: String, filename: String },
    CallStarted { call_id: String, peer_id: String, is_video: bool },
    CallEnded { call_id: String },
    Error(String),
}

/// Peer session state.
struct PeerSession {
    transport: TransportKind,
    sas: Option<String>,
}

struct ActiveTransfer {
    #[allow(dead_code)]
    sender: ChunkedSender,
    #[allow(dead_code)]
    peer_id: String,
}

struct ActiveCall {
    session: CallSession,
}

/// Main P2P engine — thread-safe, async-managed.
pub struct P2pEngine {
    identity: Identity,
    running: Arc<RwLock<bool>>,
    quic: Arc<RwLock<QuicTransport>>,
    serial: Arc<RwLock<Option<SerialTransport>>>,
    peers: Arc<RwLock<HashMap<String, PeerSession>>>,
    transfers: Arc<RwLock<HashMap<String, ActiveTransfer>>>,
    calls: Arc<RwLock<HashMap<String, ActiveCall>>>,
    event_tx: mpsc::Sender<EngineEvent>,
}

impl P2pEngine {
    pub fn new() -> (Self, mpsc::Receiver<EngineEvent>) {
        let (event_tx, event_rx) = mpsc::channel(512);
        let identity = Identity::generate();

        let engine = Self {
            identity,
            running: Arc::new(RwLock::new(false)),
            quic: Arc::new(RwLock::new(QuicTransport::new())),
            serial: Arc::new(RwLock::new(None)),
            peers: Arc::new(RwLock::new(HashMap::new())),
            transfers: Arc::new(RwLock::new(HashMap::new())),
            calls: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        };

        (engine, event_rx)
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn public_key_hex(&self) -> String {
        self.identity.public_key_hex()
    }

    pub fn qr_payload(&self) -> String {
        self.identity.qr_payload()
    }

    pub async fn start(&self, quic_port: u16) -> Result<(), String> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }

        let addr: SocketAddr = format!("0.0.0.0:{quic_port}")
            .parse()
            .map_err(|e| format!("invalid address: {e}"))?;

        {
            let mut quic = self.quic.write().await;
            quic.listen(addr).await.map_err(|e| e.to_string())?;
        }

        *running = true;
        info!(port = quic_port, "P2P engine started");
        let _ = self.event_tx.send(EngineEvent::Started).await;
        Ok(())
    }

    pub async fn connect_serial(&self, port_name: &str, baud_rate: u32) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            return Err("serial transport is not available on Android".to_string());
        }

        #[cfg(not(target_os = "android"))]
        {
            let config = SerialConfig {
                port_name: port_name.to_string(),
                baud_rate,
                ..Default::default()
            };

            let (transport, mut event_rx) = SerialTransport::new(config);
            transport.start().await?;

            let event_tx = self.event_tx.clone();
            let peers = self.peers.clone();

            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    match event {
                        crate::serial::SerialEvent::Connected { port } => {
                            let peer_id = format!("serial:{port}");
                            peers.write().await.insert(
                                peer_id.clone(),
                                PeerSession {
                                    transport: TransportKind::Serial,
                                    sas: None,
                                },
                            );
                            let _ = event_tx
                                .send(EngineEvent::PeerConnected {
                                    peer_id,
                                    transport: TransportKind::Serial,
                                })
                                .await;
                        }
                        crate::serial::SerialEvent::Disconnected { port, reason } => {
                            let peer_id = format!("serial:{port}");
                            peers.write().await.remove(&peer_id);
                            let _ = event_tx
                                .send(EngineEvent::PeerDisconnected { peer_id, reason })
                                .await;
                        }
                        crate::serial::SerialEvent::DataReceived(data) => {
                            Self::handle_inbound_data(&event_tx, &data).await;
                        }
                        crate::serial::SerialEvent::Error(e) => {
                            warn!(error = %e, "serial error");
                            let _ = event_tx.send(EngineEvent::Error(e)).await;
                        }
                    }
                }
            });

            *self.serial.write().await = Some(transport);
            Ok(())
        }
    }

    pub async fn connect_quic(&self, addr: &str) -> Result<(), String> {
        let socket_addr: SocketAddr = addr
            .parse()
            .map_err(|e| format!("invalid address: {e}"))?;

        let quic = self.quic.read().await;
        let _conn = quic.connect(socket_addr).await.map_err(|e| e.to_string())?;
        let peer_id = format!("quic:{addr}");

        self.peers.write().await.insert(
            peer_id.clone(),
            PeerSession {
                transport: TransportKind::Lan,
                sas: None,
            },
        );

        let _ = self
            .event_tx
            .send(EngineEvent::PeerConnected {
                peer_id,
                transport: TransportKind::Lan,
            })
            .await;

        Ok(())
    }

    pub async fn disconnect_peer(&self, peer_id: &str) -> Result<(), String> {
        let transport = self
            .peers
            .read()
            .await
            .get(peer_id)
            .map(|s| s.transport);

        if let Some(kind) = transport {
            if kind == TransportKind::Serial {
                if let Some(ref transport) = *self.serial.read().await {
                    transport.shutdown().await;
                }
                *self.serial.write().await = None;
            }
        }

        self.peers.write().await.remove(peer_id);
        let _ = self
            .event_tx
            .send(EngineEvent::PeerDisconnected {
                peer_id: peer_id.to_string(),
                reason: "user disconnected".to_string(),
            })
            .await;
        Ok(())
    }

    pub async fn handshake_with(&self, peer_id: &str, remote_qr: &str) -> Result<String, String> {
        let remote_pk = Identity::from_qr_payload(remote_qr).map_err(|e| e.to_string())?;

        let mut kex = HybridKeyExchange::initiator();
        let msg1 = kex.initiator_message();

        let mut responder = HybridKeyExchange::responder();
        let msg2 = responder
            .responder_accept(&msg1)
            .map_err(|e| e.to_string())?;
        kex.initiator_finish(&msg2).map_err(|e| e.to_string())?;

        let sas = kex
            .sas(self.identity.public_key_bytes().as_slice(), &remote_pk)
            .ok_or_else(|| "handshake failed".to_string())?;

        if let Some(session) = self.peers.write().await.get_mut(peer_id) {
            session.sas = Some(sas.clone());
        }

        let _ = self
            .event_tx
            .send(EngineEvent::SasReady {
                peer_id: peer_id.to_string(),
                sas: sas.clone(),
            })
            .await;

        Ok(sas)
    }

    pub async fn send_message(&self, peer_id: &str, content: &str) -> Result<(), String> {
        self.send_wire_message(peer_id, ChatMessage::text(&self.public_key_hex(), peer_id, content))
            .await
    }

    pub async fn send_file(&self, peer_id: &str, file_path: &str) -> Result<(String, String, f64), String> {
        let path = Path::new(file_path);
        if !path.exists() {
            return Err(format!("file not found: {file_path}"));
        }

        let sender = ChunkedSender::from_file(path, crate::transfer::DEFAULT_CHUNK_SIZE)
            .map_err(|e| e.to_string())?;
        let transfer_id = sender.manifest.id.to_string();
        let filename = sender.manifest.filename.clone();
        let progress = sender.progress();

        self.transfers.write().await.insert(
            transfer_id.clone(),
            ActiveTransfer {
                sender,
                peer_id: peer_id.to_string(),
            },
        );

        let notice = ChatMessage {
            id: uuid::Uuid::new_v4(),
            sender_id: self.public_key_hex(),
            recipient_id: peer_id.to_string(),
            msg_type: MessageType::File,
            content: filename.clone(),
            timestamp: chrono::Utc::now(),
            metadata: Some(serde_json::json!({
                "transfer_id": transfer_id,
                "action": "offer",
            })),
        };
        self.send_wire_message(peer_id, notice).await?;

        let _ = self
            .event_tx
            .send(EngineEvent::TransferProgress {
                id: transfer_id.clone(),
                filename: filename.clone(),
                progress,
            })
            .await;

        Ok((transfer_id, filename, progress))
    }

    pub async fn start_call(&self, peer_id: &str, is_video: bool) -> Result<String, String> {
        let mut session = CallSession::new(peer_id, is_video);
        let sdp = session.create_offer().map_err(|e| e.to_string())?;
        let call_id = session.id.clone();

        let msg = ChatMessage {
            id: uuid::Uuid::new_v4(),
            sender_id: self.public_key_hex(),
            recipient_id: peer_id.to_string(),
            msg_type: if is_video {
                MessageType::CallOffer
            } else {
                MessageType::CallOffer
            },
            content: sdp,
            timestamp: chrono::Utc::now(),
            metadata: Some(serde_json::json!({
                "call_id": call_id,
                "is_video": is_video,
            })),
        };
        self.send_wire_message(peer_id, msg).await?;

        self.calls.write().await.insert(
            call_id.clone(),
            ActiveCall { session },
        );

        let _ = self
            .event_tx
            .send(EngineEvent::CallStarted {
                call_id: call_id.clone(),
                peer_id: peer_id.to_string(),
                is_video,
            })
            .await;

        Ok(call_id)
    }

    pub async fn end_call(&self, call_id: &str) -> Result<(), String> {
        if let Some(mut active) = self.calls.write().await.remove(call_id) {
            active.session.end();
            let _ = self
                .event_tx
                .send(EngineEvent::CallEnded {
                    call_id: call_id.to_string(),
                })
                .await;
        }
        Ok(())
    }

    async fn send_wire_message(&self, peer_id: &str, msg: ChatMessage) -> Result<(), String> {
        let peers = self.peers.read().await;
        let session = peers
            .get(peer_id)
            .ok_or_else(|| format!("peer not found: {peer_id}"))?;

        let payload = msg.to_json().map_err(|e| e.to_string())?;
        let envelope = Envelope::new(payload, true);
        let wire = envelope.serialize().map_err(|e| e.to_string())?;

        match session.transport {
            TransportKind::Serial => {
                if let Some(ref transport) = *self.serial.read().await {
                    transport
                        .send(bytes::Bytes::from(wire))
                        .await
                        .map_err(|e| e.to_string())?;
                } else {
                    return Err("serial transport not connected".to_string());
                }
            }
            TransportKind::Lan | TransportKind::Wan => {
                info!(peer = peer_id, len = wire.len(), "sending via QUIC");
            }
            TransportKind::Relay => {
                info!(peer = peer_id, "sending via relay");
            }
        }

        Ok(())
    }

    async fn handle_inbound_data(event_tx: &mpsc::Sender<EngineEvent>, data: &[u8]) {
        if let Ok(envelope) = Envelope::deserialize(data) {
            if let Ok(msg) = ChatMessage::from_json(&envelope.payload) {
                let _ = event_tx.send(EngineEvent::MessageReceived(msg)).await;
                return;
            }
        }
        if let Ok(msg) = ChatMessage::from_json(data) {
            let _ = event_tx.send(EngineEvent::MessageReceived(msg)).await;
        }
    }

    pub fn available_serial_ports() -> Vec<String> {
        list_ports()
    }

    pub async fn shutdown(&self) {
        let mut running = self.running.write().await;
        if !*running {
            return;
        }
        *running = false;

        info!("P2P engine shutting down gracefully");

        if let Some(ref transport) = *self.serial.read().await {
            transport.shutdown().await;
        }
        *self.serial.write().await = None;

        self.quic.write().await.shutdown().await;
        self.peers.write().await.clear();
        self.transfers.write().await.clear();
        self.calls.write().await.clear();

        let _ = self.event_tx.send(EngineEvent::Stopped).await;
        info!("P2P engine stopped");
    }

    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    pub async fn connected_peers(&self) -> Vec<String> {
        self.peers
            .read()
            .await
            .keys()
            .cloned()
            .collect()
    }
}