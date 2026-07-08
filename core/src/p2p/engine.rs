//! Central P2P engine coordinating transports, crypto sessions, and messaging.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use quinn::Connection;
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{info, warn};

use crate::crypto::handshake::HybridKeyExchange;
use crate::crypto::identity::{parse_qr_payload, Identity};
use crate::crypto::peer_crypto::PeerCrypto;
use crate::crypto::wire::{EncryptedPayload, SignedHandshake, WireFrame};
use crate::network::local::{detect_lan_ip, local_endpoint};
use crate::network::TransportKind;
use crate::network::QuicTransport;
use crate::protocol::{ChatMessage, MessageType};
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

struct PeerSession {
    transport: TransportKind,
    crypto: PeerCrypto,
    /// In-flight hybrid KEX state for initiator path.
    pending_kex: Option<HybridKeyExchange>,
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
    /// Handshake step-2/3 waiters keyed by peer_id.
    handshake_wait: Arc<RwLock<HashMap<String, oneshot::Sender<SignedHandshake>>>>,
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
            handshake_wait: Arc::new(RwLock::new(HashMap::new())),
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
        let host = detect_lan_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        self.identity
            .qr_payload_with_endpoint(Some(&host), 9473)
    }

    pub fn local_endpoint(&self) -> Option<String> {
        local_endpoint(9473)
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

        self.spawn_quic_accept_loop();

        *running = true;
        info!(port = quic_port, "P2P engine started");
        let _ = self.event_tx.send(EngineEvent::Started).await;
        Ok(())
    }

    fn spawn_quic_accept_loop(&self) {
        let quic = self.quic.clone();
        let event_tx = self.event_tx.clone();
        let peers = self.peers.clone();
        let running = self.running.clone();
        let engine_self = self.clone_for_inbound();

        tokio::spawn(async move {
            loop {
                if !*running.read().await {
                    break;
                }

                let accepted = {
                    let transport = quic.read().await;
                    match transport.try_accept().await {
                        Ok(result) => result,
                        Err(e) => {
                            warn!(error = %e, "QUIC accept failed");
                            break;
                        }
                    }
                };

                let Some((conn, remote)) = accepted else {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };

                let peer_id = format!("quic:{remote}");
                quic.read().await.register(peer_id.clone(), conn.clone()).await;
                engine_self.spawn_quic_read_loop(peer_id.clone(), conn);

                peers.write().await.insert(
                    peer_id.clone(),
                    PeerSession {
                        transport: TransportKind::Lan,
                        crypto: PeerCrypto::new_connected(),
                        pending_kex: None,
                    },
                );

                let _ = event_tx
                    .send(EngineEvent::PeerConnected {
                        peer_id,
                        transport: TransportKind::Lan,
                    })
                    .await;
            }
        });
    }

    fn clone_for_inbound(&self) -> EngineInbound {
        EngineInbound {
            identity: self.identity.clone(),
            quic: self.quic.clone(),
            serial: self.serial.clone(),
            peers: self.peers.clone(),
            event_tx: self.event_tx.clone(),
            handshake_wait: self.handshake_wait.clone(),
        }
    }

    fn spawn_quic_read_loop(&self, peer_id: String, conn: Connection) {
        self.clone_for_inbound()
            .spawn_quic_read_loop(peer_id, conn);
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

            let inbound = self.clone_for_inbound();
            let peers = self.peers.clone();
            let active_serial_peer: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));

            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    match event {
                        crate::serial::SerialEvent::Connected { port } => {
                            let peer_id = format!("serial:{port}");
                            *active_serial_peer.write().await = Some(peer_id.clone());
                            peers.write().await.insert(
                                peer_id.clone(),
                                PeerSession {
                                    transport: TransportKind::Serial,
                                    crypto: PeerCrypto::new_connected(),
                                    pending_kex: None,
                                },
                            );
                            let _ = inbound
                                .event_tx
                                .send(EngineEvent::PeerConnected {
                                    peer_id,
                                    transport: TransportKind::Serial,
                                })
                                .await;
                        }
                        crate::serial::SerialEvent::Disconnected { port, reason } => {
                            let peer_id = format!("serial:{port}");
                            *active_serial_peer.write().await = None;
                            peers.write().await.remove(&peer_id);
                            let _ = inbound
                                .event_tx
                                .send(EngineEvent::PeerDisconnected { peer_id, reason })
                                .await;
                        }
                        crate::serial::SerialEvent::DataReceived(data) => {
                            if let Some(ref pid) = *active_serial_peer.read().await {
                                inbound.handle_inbound_bytes(pid, &data).await;
                            }
                        }
                        crate::serial::SerialEvent::Error(e) => {
                            warn!(error = %e, "serial error");
                            let _ = inbound.event_tx.send(EngineEvent::Error(e)).await;
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
        let conn = quic.connect(socket_addr).await.map_err(|e| e.to_string())?;
        let peer_id = format!("quic:{addr}");

        quic.register(peer_id.clone(), conn.clone()).await;
        self.spawn_quic_read_loop(peer_id.clone(), conn);

        self.peers.write().await.insert(
            peer_id.clone(),
            PeerSession {
                transport: TransportKind::Lan,
                crypto: PeerCrypto::new_connected(),
                pending_kex: None,
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
            match kind {
                TransportKind::Serial => {
                    if let Some(ref transport) = *self.serial.read().await {
                        transport.shutdown().await;
                    }
                    *self.serial.write().await = None;
                }
                TransportKind::Lan | TransportKind::Wan => {
                    self.quic.read().await.unregister(peer_id).await;
                }
                TransportKind::Relay => {}
            }
        }

        self.peers.write().await.remove(peer_id);
        self.handshake_wait.write().await.remove(peer_id);
        let _ = self
            .event_tx
            .send(EngineEvent::PeerDisconnected {
                peer_id: peer_id.to_string(),
                reason: "user disconnected".to_string(),
            })
            .await;
        Ok(())
    }

    pub async fn connect_and_verify(&self, remote_qr: &str) -> Result<(String, String), String> {
        let parsed = parse_qr_payload(remote_qr)
            .map_err(|e| format!("invalid peer QR: {e}"))?;

        let mut peer_id = self.connected_peers().await.into_iter().next();

        if peer_id.is_none() {
            if let Some(ref endpoint) = parsed.endpoint {
                self.connect_quic(endpoint).await?;
                peer_id = Some(format!("quic:{endpoint}"));
            }
        }

        let peer_id = peer_id.ok_or_else(|| {
            "Could not reach peer. Ensure both apps are running on the same network, \
             and use a QR code generated by a running SRLTCP app."
                .to_string()
        })?;

        let sas = self.handshake_with(&peer_id, remote_qr).await?;
        Ok((peer_id, sas))
    }

    /// Run hybrid KEX over the wire; remote QR identity must match signed handshake.
    pub async fn handshake_with(&self, peer_id: &str, remote_qr: &str) -> Result<String, String> {
        let parsed = parse_qr_payload(remote_qr).map_err(|e| e.to_string())?;
        self.handshake_with_qr_bytes(peer_id, &parsed.public_key)
            .await
    }

    async fn handshake_with_qr_bytes(
        &self,
        peer_id: &str,
        expected_remote_pk: &[u8; 32],
    ) -> Result<String, String> {
        if !self.peers.read().await.contains_key(peer_id) {
            return Err(format!("peer not connected: {peer_id}"));
        }

        let (tx, rx) = oneshot::channel();
        self.handshake_wait
            .write()
            .await
            .insert(peer_id.to_string(), tx);

        let frame1 = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(peer_id)
                .ok_or_else(|| format!("peer not found: {peer_id}"))?;
            let (mut frame, kex) = session.crypto.begin_initiator();
            frame = PeerCrypto::sign_handshake(&self.identity, 1, frame.body);
            session.pending_kex = Some(kex);
            frame
        };

        let wire = WireFrame::Handshake(frame1);
        self.send_wire_frame(peer_id, &wire).await?;

        let resp = tokio::time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| "handshake timed out waiting for peer".to_string())?
            .map_err(|_| "handshake channel closed".to_string())?;

        if resp.step != 2 {
            return Err(format!("unexpected handshake step {}", resp.step));
        }

        let (finish_frame, sas) = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(peer_id)
                .ok_or_else(|| format!("peer not found: {peer_id}"))?;
            let mut kex = session
                .pending_kex
                .take()
                .ok_or_else(|| "handshake state lost".to_string())?;
            let (finish, sas) = session.crypto.initiator_finish(
                &self.identity,
                &mut kex,
                &resp,
                expected_remote_pk,
            )?;
            (finish, sas)
        };

        let wire = WireFrame::Handshake(finish_frame);
        self.send_wire_frame(peer_id, &wire).await?;

        let _ = self
            .event_tx
            .send(EngineEvent::SasReady {
                peer_id: peer_id.to_string(),
                sas: sas.clone(),
            })
            .await;

        Ok(sas)
    }

    /// User explicitly confirms SAS — required before messaging.
    pub async fn confirm_peer_trusted(&self, peer_id: &str) -> Result<(), String> {
        let mut peers = self.peers.write().await;
        let session = peers
            .get_mut(peer_id)
            .ok_or_else(|| format!("peer not found: {peer_id}"))?;
        session.crypto.confirm_trusted()
    }

    pub async fn is_peer_trusted(&self, peer_id: &str) -> bool {
        self.peers
            .read()
            .await
            .get(peer_id)
            .map(|s| s.crypto.is_trusted())
            .unwrap_or(false)
    }

    pub async fn send_message(&self, peer_id: &str, content: &str) -> Result<(), String> {
        self.send_wire_message(
            peer_id,
            ChatMessage::text(&self.public_key_hex(), peer_id, content),
        )
        .await
    }

    pub async fn send_file(
        &self,
        peer_id: &str,
        file_path: &str,
    ) -> Result<(String, String, f64), String> {
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
            msg_type: MessageType::CallOffer,
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

    async fn send_wire_frame(&self, peer_id: &str, frame: &WireFrame) -> Result<(), String> {
        let wire = frame.serialize().map_err(|e| e.to_string())?;
        self.send_raw(peer_id, &wire).await
    }

    async fn send_wire_message(&self, peer_id: &str, msg: ChatMessage) -> Result<(), String> {
        let plaintext = msg.to_json().map_err(|e| e.to_string())?;

        let ciphertext = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(peer_id)
                .ok_or_else(|| format!("peer not found: {peer_id}"))?;
            session.crypto.encrypt(&plaintext)?
        };

        let payload = EncryptedPayload {
            version: 2,
            ciphertext,
        };
        let wire = WireFrame::Encrypted(payload);
        self.send_wire_frame(peer_id, &wire).await
    }

    async fn send_raw(&self, peer_id: &str, wire: &[u8]) -> Result<(), String> {
        let transport = self
            .peers
            .read()
            .await
            .get(peer_id)
            .map(|s| s.transport)
            .ok_or_else(|| format!("peer not found: {peer_id}"))?;

        match transport {
            TransportKind::Serial => {
                if let Some(ref transport) = *self.serial.read().await {
                    transport
                        .send(bytes::Bytes::from(wire.to_vec()))
                        .await
                        .map_err(|e| e.to_string())?;
                } else {
                    return Err("serial transport not connected".to_string());
                }
            }
            TransportKind::Lan | TransportKind::Wan => {
                self.quic
                    .read()
                    .await
                    .send(peer_id, wire)
                    .await
                    .map_err(|e| e.to_string())?;
                info!(peer = peer_id, len = wire.len(), "sent wire frame");
            }
            TransportKind::Relay => {
                info!(peer = peer_id, "relay send not implemented");
            }
        }
        Ok(())
    }

    pub fn available_serial_ports() -> Vec<crate::serial::SerialPortEntry> {
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
        self.handshake_wait.write().await.clear();

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

/// Lightweight handle for inbound dispatch (shared across read loops).
#[derive(Clone)]
struct EngineInbound {
    identity: Identity,
    quic: Arc<RwLock<QuicTransport>>,
    serial: Arc<RwLock<Option<SerialTransport>>>,
    peers: Arc<RwLock<HashMap<String, PeerSession>>>,
    event_tx: mpsc::Sender<EngineEvent>,
    handshake_wait: Arc<RwLock<HashMap<String, oneshot::Sender<SignedHandshake>>>>,
}

impl EngineInbound {
    fn spawn_quic_read_loop(&self, peer_id: String, conn: Connection) {
        let inbound = self.clone();
        tokio::spawn(async move {
            loop {
                match conn.accept_bi().await {
                    Ok((_send, mut recv)) => match recv.read_to_end(16 * 1024 * 1024).await {
                        Ok(data) if !data.is_empty() => {
                            inbound.handle_inbound_bytes(&peer_id, &data).await;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            warn!(error = %e, "QUIC stream read error");
                            break;
                        }
                    },
                    Err(quinn::ConnectionError::ApplicationClosed { .. })
                    | Err(quinn::ConnectionError::LocallyClosed) => break,
                    Err(e) => {
                        warn!(error = %e, "QUIC accept_bi error");
                        break;
                    }
                }
            }
        });
    }

    async fn handle_inbound_bytes(&self, peer_id: &str, data: &[u8]) {
        if let Ok(frame) = WireFrame::deserialize(data) {
            self.handle_wire_frame(peer_id, frame).await;
            return;
        }
        // Legacy plaintext JSON fallback (pre-0.2.9 peers)
        if let Ok(msg) = ChatMessage::from_json(data) {
            let _ = self.event_tx.send(EngineEvent::MessageReceived(msg)).await;
        }
    }

    async fn handle_wire_frame(&self, peer_id: &str, frame: WireFrame) {
        match frame {
            WireFrame::Handshake(hs) => self.handle_handshake_frame(peer_id, hs).await,
            WireFrame::Encrypted(enc) => self.handle_encrypted_frame(peer_id, enc).await,
        }
    }

    async fn handle_handshake_frame(&self, peer_id: &str, hs: SignedHandshake) {
        match hs.step {
            1 => self.handle_handshake_init(peer_id, hs).await,
            2 => {
                if let Some(tx) = self.handshake_wait.write().await.remove(peer_id) {
                    let _ = tx.send(hs);
                }
            }
            3 => self.handle_handshake_finish(peer_id, hs).await,
            _ => warn!(step = hs.step, "unknown handshake step"),
        }
    }

    async fn handle_handshake_init(&self, peer_id: &str, init: SignedHandshake) {
        let resp = async {
            let mut peers = self.peers.write().await;
            if !peers.contains_key(peer_id) {
                peers.insert(
                    peer_id.to_string(),
                    PeerSession {
                        transport: TransportKind::Lan,
                        crypto: PeerCrypto::new_connected(),
                        pending_kex: None,
                    },
                );
            }
            let session = peers.get_mut(peer_id).unwrap();
            let (resp, kex) = session.crypto.responder_accept(&self.identity, &init)?;
            session.pending_kex = Some(kex);
            Ok::<SignedHandshake, String>(resp)
        }
        .await;

        match resp {
            Ok(resp) => {
                let wire = WireFrame::Handshake(resp);
                if let Err(e) = self.send_raw_frame(peer_id, &wire).await {
                    warn!(error = %e, "failed to send handshake response");
                }
            }
            Err(e) => {
                let _ = self.event_tx.send(EngineEvent::Error(e)).await;
            }
        }
    }

    async fn handle_handshake_finish(&self, peer_id: &str, finish: SignedHandshake) {
        let sas_result = async {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(peer_id)
                .ok_or_else(|| format!("peer not found: {peer_id}"))?;
            let kex = session
                .pending_kex
                .take()
                .ok_or_else(|| "responder kex state missing".to_string())?;
            session.crypto.responder_finish(&self.identity, &kex, &finish)
        }
        .await;

        match sas_result {
            Ok(sas) => {
                let _ = self
                    .event_tx
                    .send(EngineEvent::SasReady {
                        peer_id: peer_id.to_string(),
                        sas: sas.clone(),
                    })
                    .await;
            }
            Err(e) => {
                let _ = self.event_tx.send(EngineEvent::Error(e)).await;
            }
        }
    }

    async fn handle_encrypted_frame(&self, peer_id: &str, enc: EncryptedPayload) {
        let plaintext = match async {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(peer_id)
                .ok_or_else(|| format!("peer not found: {peer_id}"))?;
            session.crypto.decrypt(&enc.ciphertext)
        }
        .await
        {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "decrypt failed");
                return;
            }
        };

        if let Ok(msg) = ChatMessage::from_json(&plaintext) {
            let _ = self.event_tx.send(EngineEvent::MessageReceived(msg)).await;
        }
    }

    async fn send_raw_frame(&self, peer_id: &str, frame: &WireFrame) -> Result<(), String> {
        let wire = frame.serialize().map_err(|e| e.to_string())?;
        let transport = self
            .peers
            .read()
            .await
            .get(peer_id)
            .map(|s| s.transport)
            .ok_or_else(|| format!("peer not found: {peer_id}"))?;

        match transport {
            TransportKind::Serial => {
                if let Some(ref transport) = *self.serial.read().await {
                    transport
                        .send(bytes::Bytes::from(wire))
                        .await
                        .map_err(|e| e.to_string())?;
                } else {
                    return Err("serial transport not connected".into());
                }
            }
            TransportKind::Lan | TransportKind::Wan => {
                self.quic
                    .read()
                    .await
                    .send(peer_id, &wire)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            TransportKind::Relay => {}
        }
        Ok(())
    }
}

