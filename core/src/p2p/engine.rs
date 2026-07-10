//! Central P2P engine coordinating transports, crypto sessions, and messaging.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::Connection;
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{info, warn};

use crate::crypto::handshake::HybridKeyExchange;
use crate::crypto::identity::{parse_qr_payload, Identity};
use crate::crypto::peer_crypto::PeerCrypto;
use crate::crypto::wire::{EncryptedPayload, SignedHandshake, WireFrame};
use crate::network::TransportKind;
use crate::network::IrohTransport;
use crate::protocol::{ChatMessage, MessageType};
use crate::serial::{list_ports, SerialConfig, SerialTransport};
use crate::transfer::{ChunkAck, ChunkedReceiver, ChunkedSender, TransferManifest};
use crate::webrtc::CallSession;

fn peer_id_from_pubkey(pk: &[u8; 32]) -> String {
    format!("peer:{}", hex::encode(pk))
}

fn iroh_peer_id(node_id: impl std::fmt::Display) -> String {
    format!("iroh:{node_id}")
}

/// Resolve transient transport ids to canonical peer session keys.
async fn resolve_session_peer(
    peers: &Arc<RwLock<HashMap<String, PeerSession>>>,
    aliases: &Arc<RwLock<HashMap<String, String>>>,
    peer_id: &str,
) -> Result<String, String> {
    if let Some(canonical) = aliases.read().await.get(peer_id) {
        return Ok(canonical.clone());
    }
    let peers = peers.read().await;
    if peers.contains_key(peer_id) {
        return Ok(peer_id.to_string());
    }
    if let Some(hex_id) = peer_id.strip_prefix("peer:") {
        let hex_id = hex_id.to_lowercase();
        for (id, session) in peers.iter() {
            if hex::encode(session.crypto.remote_identity).to_lowercase() == hex_id {
                return Ok(id.clone());
            }
        }
    }
    Err(format!("peer not connected: {peer_id}"))
}

/// Engine lifecycle events for UI/bindings.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    Started,
    Stopped,
    PeerConnected { peer_id: String, transport: TransportKind },
    PeerDisconnected { peer_id: String, reason: String },
    MessageReceived(ChatMessage),
    SasReady {
        peer_id: String,
        sas: String,
        auto_trusted: bool,
    },
    PeerIdUpdated { old_id: String, new_id: String },
    TransferProgress {
        id: String,
        filename: String,
        progress: f64,
        peer_id: String,
    },
    TransferComplete {
        id: String,
        filename: String,
        peer_id: String,
        path: String,
    },
    TransferCancelled {
        id: String,
        filename: String,
        peer_id: String,
    },
    CallSignaling {
        call_id: String,
        peer_id: String,
        signal: String,
        payload: String,
        is_video: bool,
    },
    CallEnded { call_id: String },
    MessageQueued { peer_id: String, queue_size: usize },
    Reconnecting { peer_id: String },
    PeerProfile { peer_id: String, display_name: String },
    Error(String),
}

const MAX_PENDING_PER_PEER: usize = 500;

#[derive(Debug, Clone)]
enum PendingOutbound {
    Text(String),
    File(PathBuf),
}

struct PeerSession {
    transport: TransportKind,
    crypto: PeerCrypto,
    /// In-flight hybrid KEX state for initiator path.
    pending_kex: Option<HybridKeyExchange>,
}

struct ActiveTransfer {
    sender: ChunkedSender,
    peer_id: String,
}

struct ActiveCall {
    session: CallSession,
}

/// Main P2P engine — thread-safe, async-managed.
pub struct P2pEngine {
    identity: Identity,
    running: Arc<RwLock<bool>>,
    iroh: Arc<RwLock<IrohTransport>>,
    serial: Arc<RwLock<Option<SerialTransport>>>,
    peers: Arc<RwLock<HashMap<String, PeerSession>>>,
    transfers: Arc<RwLock<HashMap<String, ActiveTransfer>>>,
    calls: Arc<RwLock<HashMap<String, ActiveCall>>>,
    event_tx: mpsc::Sender<EngineEvent>,
    /// Handshake step-2/3 waiters keyed by peer_id.
    handshake_wait: Arc<RwLock<HashMap<String, oneshot::Sender<SignedHandshake>>>>,
    /// Maps transient transport ids (e.g. iroh:…) to canonical peer:{pubkey}.
    peer_aliases: Arc<RwLock<HashMap<String, String>>>,
    /// Previously verified peer Ed25519 public keys (hex).
    trusted_pubkeys: Arc<RwLock<HashSet<String>>>,
    incoming_transfers: Arc<RwLock<HashMap<String, ChunkedReceiver>>>,
    receive_dir: Arc<RwLock<PathBuf>>,
    /// Outbound messages/files waiting for reconnect.
    pending_outbound: Arc<RwLock<HashMap<String, VecDeque<PendingOutbound>>>>,
    /// Saved peer QR payloads for seamless auto-reconnect.
    saved_peer_qr: Arc<RwLock<HashMap<String, String>>>,
    cancelled_transfers: Arc<RwLock<HashSet<String>>>,
    user_paused: Arc<RwLock<HashSet<String>>>,
    local_display_name: Arc<RwLock<String>>,
    remote_display_names: Arc<RwLock<HashMap<String, String>>>,
}

impl P2pEngine {
    pub fn new() -> (Self, mpsc::Receiver<EngineEvent>) {
        let (event_tx, event_rx) = mpsc::channel(512);
        let identity = Identity::generate();

        let engine = Self {
            identity,
            running: Arc::new(RwLock::new(false)),
            iroh: Arc::new(RwLock::new(IrohTransport::new())),
            serial: Arc::new(RwLock::new(None)),
            peers: Arc::new(RwLock::new(HashMap::new())),
            transfers: Arc::new(RwLock::new(HashMap::new())),
            calls: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            handshake_wait: Arc::new(RwLock::new(HashMap::new())),
            peer_aliases: Arc::new(RwLock::new(HashMap::new())),
            trusted_pubkeys: Arc::new(RwLock::new(HashSet::new())),
            incoming_transfers: Arc::new(RwLock::new(HashMap::new())),
            receive_dir: Arc::new(RwLock::new(
                std::env::temp_dir().join("srltcp_received"),
            )),
            pending_outbound: Arc::new(RwLock::new(HashMap::new())),
            saved_peer_qr: Arc::new(RwLock::new(HashMap::new())),
            cancelled_transfers: Arc::new(RwLock::new(HashSet::new())),
            user_paused: Arc::new(RwLock::new(HashSet::new())),
            local_display_name: Arc::new(RwLock::new(String::new())),
            remote_display_names: Arc::new(RwLock::new(HashMap::new())),
        };

        (engine, event_rx)
    }

    pub async fn set_receive_dir(&self, path: PathBuf) {
        *self.receive_dir.write().await = path;
    }

    pub async fn receive_dir(&self) -> PathBuf {
        self.receive_dir.read().await.clone()
    }

    pub async fn load_trusted_pubkeys(&self, pubkeys: Vec<String>) {
        let mut set = self.trusted_pubkeys.write().await;
        set.clear();
        for pk in pubkeys {
            let trimmed = pk.trim().to_lowercase();
            if !trimmed.is_empty() {
                set.insert(trimmed);
            }
        }
    }

    async fn resolve_peer_id(&self, peer_id: &str) -> Result<String, String> {
        resolve_session_peer(&self.peers, &self.peer_aliases, peer_id).await
    }

    async fn register_peer_alias(&self, old_id: &str, canonical: &str) {
        if old_id != canonical {
            self.peer_aliases
                .write()
                .await
                .insert(old_id.to_string(), canonical.to_string());
        }
    }

    fn pubkey_hex_from_peer_id(peer_id: &str) -> Option<String> {
        peer_id
            .strip_prefix("peer:")
            .map(|h| h.to_lowercase())
            .filter(|h| !h.is_empty())
    }

    pub async fn register_saved_peer(&self, peer_id: &str, qr: &str) {
        if let Ok(canonical) = self.resolve_peer_id(peer_id).await {
            self.saved_peer_qr
                .write()
                .await
                .insert(canonical, qr.trim().to_string());
        } else if peer_id.starts_with("peer:") {
            self.saved_peer_qr
                .write()
                .await
                .insert(peer_id.to_string(), qr.trim().to_string());
        }
    }

    async fn can_queue_for_peer(&self, peer_id: &str) -> bool {
        if let Some(hex) = Self::pubkey_hex_from_peer_id(peer_id) {
            return self.trusted_pubkeys.read().await.contains(&hex);
        }
        false
    }

    async fn enqueue_pending(&self, peer_id: &str, item: PendingOutbound) -> Result<usize, String> {
        let mut map = self.pending_outbound.write().await;
        let q = map.entry(peer_id.to_string()).or_default();
        if q.len() >= MAX_PENDING_PER_PEER {
            return Err("outbound queue full — wait for reconnect".into());
        }
        q.push_back(item);
        Ok(q.len())
    }

    async fn flush_pending_for_peer(&self, peer_id: &str) {
        let items: Vec<PendingOutbound> = {
            let mut map = self.pending_outbound.write().await;
            map.remove(peer_id)
                .map(|q| q.into_iter().collect())
                .unwrap_or_default()
        };
        let mut deferred = Vec::new();
        for item in items {
            let err = match &item {
                PendingOutbound::Text(content) => self.try_send_message_now(peer_id, content).await.err(),
                PendingOutbound::File(path) => {
                    let path_str = path.to_string_lossy().to_string();
                    self.try_send_file_now(peer_id, &path_str).await.err().map(|e| e)
                }
            };
            if err.as_ref().is_some_and(|e| e.contains("ratchet send chain not ready")) {
                deferred.push(item);
            }
        }
        if !deferred.is_empty() {
            let mut map = self.pending_outbound.write().await;
            let q = map.entry(peer_id.to_string()).or_default();
            for item in deferred {
                q.push_back(item);
            }
        }
    }

    /// Initiator opens the ratchet send chain so the responder can reply (Signal spec).
    async fn send_ratchet_bootstrap(&self, peer_id: &str) -> Result<(), String> {
        let resolved = self.resolve_peer_id(peer_id).await?;
        let is_initiator = self
            .peers
            .read()
            .await
            .get(&resolved)
            .is_some_and(|s| s.crypto.is_initiator());
        if !is_initiator {
            return Ok(());
        }
        let msg = ChatMessage {
            id: uuid::Uuid::new_v4(),
            sender_id: self.public_key_hex(),
            recipient_id: resolved.clone(),
            msg_type: MessageType::System,
            content: String::new(),
            timestamp: chrono::Utc::now(),
            metadata: Some(serde_json::json!({ "action": "ratchet_open" })),
        };
        self.send_wire_message(&resolved, msg).await
    }

    fn schedule_auto_reconnect(&self, peer_id: String) {
        let engine = self.clone_engine();
        tokio::spawn(async move {
            let qr = engine.saved_peer_qr.read().await.get(&peer_id).cloned();
            let Some(qr) = qr else { return };
            if !engine.can_queue_for_peer(&peer_id).await {
                return;
            }
            for attempt in 0..6u32 {
                let delay = Duration::from_secs(2u64.pow(attempt.min(4)));
                tokio::time::sleep(delay).await;
                let _ = engine
                    .event_tx
                    .send(EngineEvent::Reconnecting {
                        peer_id: peer_id.clone(),
                    })
                    .await;
                match engine.connect_and_verify(&qr).await {
                    Ok((id, _, auto_trusted)) => {
                        if auto_trusted {
                            engine.flush_pending_for_peer(&id).await;
                        }
                        return;
                    }
                    Err(_) => continue,
                }
            }
        });
    }

    fn clone_engine(&self) -> Self {
        Self {
            identity: self.identity.clone(),
            running: self.running.clone(),
            iroh: self.iroh.clone(),
            serial: self.serial.clone(),
            peers: self.peers.clone(),
            transfers: self.transfers.clone(),
            calls: self.calls.clone(),
            event_tx: self.event_tx.clone(),
            handshake_wait: self.handshake_wait.clone(),
            peer_aliases: self.peer_aliases.clone(),
            trusted_pubkeys: self.trusted_pubkeys.clone(),
            incoming_transfers: self.incoming_transfers.clone(),
            receive_dir: self.receive_dir.clone(),
            pending_outbound: self.pending_outbound.clone(),
            saved_peer_qr: self.saved_peer_qr.clone(),
            cancelled_transfers: self.cancelled_transfers.clone(),
            user_paused: self.user_paused.clone(),
            local_display_name: self.local_display_name.clone(),
            remote_display_names: self.remote_display_names.clone(),
        }
    }

    pub async fn set_display_name(&self, name: &str) {
        *self.local_display_name.write().await = name.trim().to_string();
    }

    pub async fn get_display_name(&self, peer_id: &str) -> Option<String> {
        if let Ok(resolved) = self.resolve_peer_id(peer_id).await {
            return self.remote_display_names.read().await.get(&resolved).cloned();
        }
        self.remote_display_names.read().await.get(peer_id).cloned()
    }

    pub async fn broadcast_profile(&self, peer_id: &str) -> Result<(), String> {
        let name = self.local_display_name.read().await.clone();
        if name.is_empty() {
            return Ok(());
        }
        let resolved = self.resolve_peer_id(peer_id).await?;
        if !self
            .peers
            .read()
            .await
            .get(&resolved)
            .is_some_and(|s| s.crypto.can_send_encrypted())
        {
            return Ok(());
        }
        let msg = ChatMessage {
            id: uuid::Uuid::new_v4(),
            sender_id: self.public_key_hex(),
            recipient_id: resolved.clone(),
            msg_type: MessageType::System,
            content: name.clone(),
            timestamp: chrono::Utc::now(),
            metadata: Some(serde_json::json!({ "action": "profile" })),
        };
        self.send_wire_message(&resolved, msg).await
    }

    fn transfer_storage_name(transfer_id: &str, filename: &str) -> String {
        let prefix = transfer_id.get(..8).unwrap_or(transfer_id);
        format!("{prefix}_{filename}")
    }

    async fn canonicalize_peer(&self, conn_peer_id: &str) -> Result<String, String> {
        let canonical = {
            let peers = self.peers.read().await;
            let session = peers
                .get(conn_peer_id)
                .ok_or_else(|| format!("peer not found: {conn_peer_id}"))?;
            if session.crypto.remote_identity == [0u8; 32] {
                return Ok(conn_peer_id.to_string());
            }
            peer_id_from_pubkey(&session.crypto.remote_identity)
        };

        if canonical == conn_peer_id {
            return Ok(canonical);
        }

        let session = {
            let mut peers = self.peers.write().await;
            peers
                .remove(conn_peer_id)
                .ok_or_else(|| format!("peer not found: {conn_peer_id}"))?
        };
        self.peers.write().await.insert(canonical.clone(), session);
        self.iroh.read().await.rekey(conn_peer_id, &canonical).await;
        self.register_peer_alias(conn_peer_id, &canonical).await;

        if let Some(tx) = self.handshake_wait.write().await.remove(conn_peer_id) {
            self.handshake_wait
                .write()
                .await
                .insert(canonical.clone(), tx);
        }

        let _ = self
            .event_tx
            .send(EngineEvent::PeerIdUpdated {
                old_id: conn_peer_id.to_string(),
                new_id: canonical.clone(),
            })
            .await;

        Ok(canonical)
    }

    async fn cleanup_sessions_for_pubkey(&self, pubkey: &[u8; 32]) {
        let canonical = peer_id_from_pubkey(pubkey);
        let stale: Vec<String> = {
            let peers = self.peers.read().await;
            peers
                .iter()
                .filter(|(id, s)| *id == &canonical || s.crypto.remote_identity == *pubkey)
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in stale {
            let _ = self.disconnect_peer(&id).await;
        }
    }

    async fn ensure_started(&self) -> Result<(), String> {
        if *self.running.read().await {
            return Ok(());
        }
        self.start(crate::DEFAULT_QUIC_PORT).await
    }

    /// Block until iroh endpoint is bound (for QR v4 generation).
    pub async fn wait_until_ready(&self, timeout_secs: u64) -> Result<(), String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
        if !*self.running.read().await {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            tokio::time::timeout(remaining, self.start(crate::DEFAULT_QUIC_PORT))
                .await
                .map_err(|_| "engine start timed out — check network and retry".to_string())??;
        }
        while std::time::Instant::now() < deadline {
            if self.iroh.read().await.is_bound() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("iroh endpoint not ready — check network connection and retry".into())
    }

    pub async fn is_ready(&self) -> bool {
        *self.running.read().await && self.iroh.read().await.is_bound()
    }

    async fn ensure_connected(&self, parsed: &crate::crypto::identity::ParsedQr) -> Result<String, String> {
        self.ensure_started().await?;

        let canonical = peer_id_from_pubkey(&parsed.public_key);
        {
            let peers = self.peers.read().await;
            if let Some(session) = peers.get(&canonical) {
                if session.crypto.is_trusted()
                    && self.iroh.read().await.has_connection(&canonical).await
                {
                    return Ok(canonical);
                }
            }
        }

        self.cleanup_sessions_for_pubkey(&parsed.public_key).await;

        if let Some(ref ticket) = parsed.iroh_ticket {
            return self.connect_iroh_ticket(ticket).await;
        }

        if parsed.endpoint.is_some() {
            return Err(
                "QR uses legacy LAN address (v3). Update both peers to v0.2.13+ and share a \
                 fresh QR with iroh NAT traversal."
                    .into(),
            );
        }

        Err(
            "QR has no iroh ticket (v4 required). Ask peer to open the app, wait until \
             online, then share a fresh QR."
                .into(),
        )
    }

    pub async fn is_peer_connected(&self, peer_id: &str) -> bool {
        if let Ok(resolved) = self.resolve_peer_id(peer_id).await {
            return self.iroh.read().await.has_connection(&resolved).await;
        }
        false
    }

    async fn maybe_auto_trust(&self, peer_id: &str, remote_pk: &[u8; 32]) -> bool {
        let pk_hex = hex::encode(remote_pk).to_lowercase();
        if !self.trusted_pubkeys.read().await.contains(&pk_hex) {
            return false;
        }
        if self.confirm_peer_trusted(peer_id).await.is_ok() {
            return true;
        }
        false
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn public_key_hex(&self) -> String {
        self.identity.public_key_hex()
    }

    pub async fn iroh_ticket(&self) -> Result<String, String> {
        self.iroh
            .read()
            .await
            .ticket_string()
            .map_err(|e| e.to_string())
    }

    /// Deprecated — use `iroh_ticket()`.
    pub async fn local_endpoint(&self) -> Option<String> {
        self.iroh_ticket().await.ok()
    }

    pub fn qr_payload(&self) -> String {
        // Sync fallback before iroh is online — identity-only v2.
        self.identity.qr_payload()
    }

    pub async fn qr_payload_async(&self) -> Result<String, String> {
        self.wait_until_ready(30).await?;
        let ticket = self.iroh_ticket().await?;
        Ok(self.identity.qr_payload_v4(&ticket))
    }

    pub async fn start(&self, _quic_port: u16) -> Result<(), String> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }

        {
            let mut transport = self.iroh.write().await;
            transport.bind().await.map_err(|e| e.to_string())?;
        }

        self.spawn_iroh_accept_loop();

        *running = true;
        info!("P2P engine started (iroh NAT traversal)");
        let _ = self.event_tx.send(EngineEvent::Started).await;
        Ok(())
    }

    fn spawn_iroh_accept_loop(&self) {
        let iroh = self.iroh.clone();
        let event_tx = self.event_tx.clone();
        let peers = self.peers.clone();
        let running = self.running.clone();
        let engine_self = self.clone_engine();

        tokio::spawn(async move {
            loop {
                if !*running.read().await {
                    break;
                }

                let accepted = {
                    let transport = iroh.read().await;
                    match transport.try_accept().await {
                        Ok(result) => result,
                        Err(e) => {
                            warn!(error = %e, "iroh accept failed");
                            break;
                        }
                    }
                };

                let Some((conn, peer_id)) = accepted else {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };

                iroh.read().await.register(peer_id.clone(), conn.clone()).await;
                engine_self.spawn_iroh_read_loop(peer_id.clone(), conn);

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
            iroh: self.iroh.clone(),
            serial: self.serial.clone(),
            peers: self.peers.clone(),
            event_tx: self.event_tx.clone(),
            handshake_wait: self.handshake_wait.clone(),
            peer_aliases: self.peer_aliases.clone(),
            trusted_pubkeys: self.trusted_pubkeys.clone(),
            incoming_transfers: self.incoming_transfers.clone(),
            receive_dir: self.receive_dir.clone(),
            transfers: self.transfers.clone(),
            pending_outbound: self.pending_outbound.clone(),
            cancelled_transfers: self.cancelled_transfers.clone(),
            user_paused: self.user_paused.clone(),
            local_display_name: self.local_display_name.clone(),
            remote_display_names: self.remote_display_names.clone(),
        }
    }

    fn spawn_iroh_read_loop(&self, peer_id: String, conn: Connection) {
        let inbound = self.clone_for_inbound();
        let engine = self.clone_engine();
        tokio::spawn(async move {
            crate::network::iroh_transport::read_connection_loop(conn, peer_id.clone(), move |pid, data| {
                let inbound = inbound.clone();
                async move {
                    inbound.handle_inbound_bytes(&pid, &data).await;
                }
            })
            .await;
            let resolved = resolve_session_peer(
                &engine.peers,
                &engine.peer_aliases,
                &peer_id,
            )
            .await
            .unwrap_or(peer_id);
            engine.on_connection_lost(&resolved).await;
        });
    }

    pub async fn connect_iroh_ticket(&self, ticket: &str) -> Result<String, String> {
        let addr = IrohTransport::parse_ticket(ticket).map_err(|e| format!("invalid iroh ticket: {e}"))?;
        self.connect_iroh_addr(addr, TransportKind::Relay).await
    }

    async fn connect_iroh_addr(
        &self,
        addr: iroh::EndpointAddr,
        kind: TransportKind,
    ) -> Result<String, String> {
        let peer_id = iroh_peer_id(addr.id);

        if self.iroh.read().await.has_connection(&peer_id).await
            && self.peers.read().await.contains_key(&peer_id)
        {
            info!(%peer_id, "reusing existing iroh connection");
            return Ok(peer_id);
        }

        let iroh = self.iroh.read().await;
        let conn = iroh.connect(addr.clone()).await.map_err(|e| {
            format!(
                "iroh dial failed ({e}). Peer may be offline, behind a strict firewall, \
                 or using an expired QR — ask them to share a fresh QR while online."
            )
        })?;

        iroh.register(peer_id.clone(), conn.clone()).await;
        drop(iroh);
        self.spawn_iroh_read_loop(peer_id.clone(), conn);

        self.peers.write().await.insert(
            peer_id.clone(),
            PeerSession {
                transport: kind,
                crypto: PeerCrypto::new_connected(),
                pending_kex: None,
            },
        );

        let _ = self
            .event_tx
            .send(EngineEvent::PeerConnected {
                peer_id: peer_id.clone(),
                transport: kind,
            })
            .await;
        Ok(peer_id)
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

    /// Deprecated — use connect_iroh_ticket. Kept for API compatibility.
    pub async fn connect_quic(&self, addr: &str) -> Result<(), String> {
        self.connect_iroh_ticket(addr).await.map(|_| ())
    }

    /// Deprecated — iroh handles WAN via NAT traversal.
    pub async fn connect_wan(&self, ticket: &str) -> Result<(), String> {
        self.connect_iroh_ticket(ticket).await.map(|_| ())
    }

    pub async fn disconnect_peer(&self, peer_id: &str) -> Result<(), String> {
        let resolved = self.resolve_peer_id(peer_id).await.unwrap_or_else(|_| peer_id.to_string());
        let transport = self
            .peers
            .read()
            .await
            .get(&resolved)
            .map(|s| s.transport);

        if let Some(kind) = transport {
            match kind {
                TransportKind::Serial => {
                    if let Some(ref transport) = *self.serial.read().await {
                        transport.shutdown().await;
                    }
                    *self.serial.write().await = None;
                }
                TransportKind::Lan | TransportKind::Wan | TransportKind::Relay => {
                    self.iroh.read().await.unregister(&resolved).await;
                }
            }
        }

        self.peers.write().await.remove(&resolved);
        self.handshake_wait.write().await.remove(&resolved);
        self.user_paused.write().await.insert(resolved.clone());
        let _ = self
            .event_tx
            .send(EngineEvent::PeerDisconnected {
                peer_id: resolved,
                reason: "user disconnected".to_string(),
            })
            .await;
        Ok(())
    }

    async fn on_connection_lost(&self, peer_id: &str) {
        if self.user_paused.read().await.contains(peer_id) {
            return;
        }
        let _ = self
            .event_tx
            .send(EngineEvent::PeerDisconnected {
                peer_id: peer_id.to_string(),
                reason: "connection lost".to_string(),
            })
            .await;
        self.schedule_auto_reconnect(peer_id.to_string());
    }

    /// Connect, run handshake, canonicalize peer id. Returns (peer_id, sas, auto_trusted).
    pub async fn connect_and_verify(
        &self,
        remote_qr: &str,
    ) -> Result<(String, String, bool), String> {
        let normalized = crate::crypto::identity::normalize_qr_input(remote_qr);
        let parsed = parse_qr_payload(&normalized)
            .map_err(|e| format!("invalid peer QR: {e}"))?;

        let canonical = peer_id_from_pubkey(&parsed.public_key);
        if self.is_peer_connected(&canonical).await && self.is_peer_trusted(&canonical).await {
            self.flush_pending_for_peer(&canonical).await;
            return Ok((canonical, String::new(), true));
        }

        let conn_peer_id = self.ensure_connected(&parsed).await?;

        // Reset crypto for a fresh handshake on this transport session.
        {
            let mut peers = self.peers.write().await;
            if let Some(session) = peers.get_mut(&conn_peer_id) {
                session.crypto = PeerCrypto::new_connected();
                session.pending_kex = None;
            }
        }

        self.handshake_with_qr_bytes(&conn_peer_id, &parsed.public_key)
            .await
    }

    /// Run hybrid KEX over the wire; remote QR identity must match signed handshake.
    pub async fn handshake_with(&self, peer_id: &str, remote_qr: &str) -> Result<String, String> {
        let parsed = parse_qr_payload(remote_qr).map_err(|e| e.to_string())?;
        let (_, sas, _) = self
            .handshake_with_qr_bytes(peer_id, &parsed.public_key)
            .await?;
        Ok(sas)
    }

    async fn handshake_with_qr_bytes(
        &self,
        peer_id: &str,
        expected_remote_pk: &[u8; 32],
    ) -> Result<(String, String, bool), String> {
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
            let (step1_body, kex) = session.crypto.begin_initiator();
            session
                .crypto
                .record_initiator_step1(&step1_body)
                .map_err(|e| format!("handshake step 1: {e}"))?;
            session.pending_kex = Some(kex);
            PeerCrypto::sign_handshake(&self.identity, 1, step1_body)
        };

        let wire = WireFrame::Handshake(frame1);
        self.send_wire_frame(peer_id, &wire).await?;

        let resp = match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(hs)) => hs,
            Ok(Err(_)) => {
                self.handshake_wait.write().await.remove(peer_id);
                return Err("handshake channel closed".into());
            }
            Err(_) => {
                self.handshake_wait.write().await.remove(peer_id);
                {
                    let mut peers = self.peers.write().await;
                    if let Some(session) = peers.get_mut(peer_id) {
                        session.crypto = PeerCrypto::new_connected();
                        session.pending_kex = None;
                    }
                }
                return Err("handshake timed out waiting for peer".into());
            }
        };

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
            let step3_body = session.crypto.initiator_process_step2(
                &mut kex,
                &resp,
                expected_remote_pk,
            )?;
            let finish = PeerCrypto::sign_handshake(&self.identity, 3, step3_body.clone());
            let sas = session
                .crypto
                .initiator_finalize_step3(&self.identity, &step3_body)?;
            (finish, sas)
        };

        let wire = WireFrame::Handshake(finish_frame);
        self.send_wire_frame(peer_id, &wire).await?;

        let canonical = self.canonicalize_peer(peer_id).await?;
        let auto_trusted = self.maybe_auto_trust(&canonical, expected_remote_pk).await;

        let _ = self
            .event_tx
            .send(EngineEvent::SasReady {
                peer_id: canonical.clone(),
                sas: sas.clone(),
                auto_trusted,
            })
            .await;

        self.user_paused.write().await.remove(&canonical);
        if auto_trusted {
            let _ = self.send_ratchet_bootstrap(&canonical).await;
            self.flush_pending_for_peer(&canonical).await;
            let _ = self.broadcast_profile(&canonical).await;
        }

        Ok((canonical, sas, auto_trusted))
    }

    /// User explicitly confirms SAS — required before messaging.
    pub async fn confirm_peer_trusted(&self, peer_id: &str) -> Result<(), String> {
        let resolved = self.resolve_peer_id(peer_id).await?;
        let remote_pk = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(&resolved)
                .ok_or_else(|| format!("peer not found: {resolved}"))?;
            session.crypto.confirm_trusted()?;
            session.crypto.remote_identity
        };
        if remote_pk != [0u8; 32] {
            self.trusted_pubkeys
                .write()
                .await
                .insert(hex::encode(remote_pk).to_lowercase());
        }
        self.user_paused.write().await.remove(&resolved);
        let _ = self.send_ratchet_bootstrap(&resolved).await;
        let _ = self.broadcast_profile(&resolved).await;
        self.flush_pending_for_peer(&resolved).await;
        Ok(())
    }

    pub async fn is_peer_trusted(&self, peer_id: &str) -> bool {
        if let Ok(resolved) = self.resolve_peer_id(peer_id).await {
            return self
                .peers
                .read()
                .await
                .get(&resolved)
                .map(|s| s.crypto.is_trusted())
                .unwrap_or(false);
        }
        false
    }

    pub async fn send_message(&self, peer_id: &str, content: &str) -> Result<(), String> {
        let resolved = self
            .resolve_peer_id(peer_id)
            .await
            .unwrap_or_else(|_| peer_id.to_string());
        match self.try_send_message_now(&resolved, content).await {
            Ok(()) => Ok(()),
            Err(e)
                if (e.contains("not connected") || e.contains("reconnect"))
                    && self.can_queue_for_peer(&resolved).await =>
            {
                let size = self
                    .enqueue_pending(&resolved, PendingOutbound::Text(content.to_string()))
                    .await?;
                let _ = self
                    .event_tx
                    .send(EngineEvent::MessageQueued {
                        peer_id: resolved,
                        queue_size: size,
                    })
                    .await;
                self.schedule_auto_reconnect(
                    self.resolve_peer_id(peer_id)
                        .await
                        .unwrap_or_else(|_| peer_id.to_string()),
                );
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    async fn try_send_message_now(&self, peer_id: &str, content: &str) -> Result<(), String> {
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
        let resolved = self
            .resolve_peer_id(peer_id)
            .await
            .unwrap_or_else(|_| peer_id.to_string());
        match self.try_send_file_now(&resolved, file_path).await {
            Ok(v) => return Ok(v),
            Err(e)
                if (e.contains("not connected") || e.contains("reconnect"))
                    && self.can_queue_for_peer(&resolved).await =>
            {
                let size = self
                    .enqueue_pending(&resolved, PendingOutbound::File(PathBuf::from(file_path)))
                    .await?;
                let _ = self
                    .event_tx
                    .send(EngineEvent::MessageQueued {
                        peer_id: resolved.clone(),
                        queue_size: size,
                    })
                    .await;
                self.schedule_auto_reconnect(resolved.clone());
                return Ok((String::new(), format!("queued:{file_path}"), 0.0));
            }
            Err(e) => return Err(e),
        }
    }

    async fn try_send_file_now(
        &self,
        peer_id: &str,
        file_path: &str,
    ) -> Result<(String, String, f64), String> {
        let resolved = self.resolve_peer_id(peer_id).await?;
        let path = Path::new(file_path);
        if !path.exists() {
            return Err(format!("file not found: {file_path}"));
        }

        let sender = ChunkedSender::from_file(path, crate::transfer::DEFAULT_CHUNK_SIZE)
            .map_err(|e| e.to_string())?;
        let transfer_id = sender.manifest.id.to_string();
        let filename = sender.manifest.filename.clone();
        let progress = sender.progress();
        let manifest = sender.manifest.clone();

        self.transfers.write().await.insert(
            transfer_id.clone(),
            ActiveTransfer {
                sender,
                peer_id: resolved.clone(),
            },
        );

        let notice = ChatMessage {
            id: uuid::Uuid::new_v4(),
            sender_id: self.public_key_hex(),
            recipient_id: resolved.clone(),
            msg_type: MessageType::File,
            content: filename.clone(),
            timestamp: chrono::Utc::now(),
            metadata: Some(serde_json::json!({
                "transfer_id": transfer_id,
                "action": "offer",
                "filename": filename,
                "total_chunks": manifest.total_chunks,
                "total_size": manifest.total_size,
                "sha256": manifest.sha256,
            })),
        };
        self.send_wire_message(&resolved, notice).await?;

        let _ = self
            .event_tx
            .send(EngineEvent::TransferProgress {
                id: transfer_id.clone(),
                filename: filename.clone(),
                progress,
                peer_id: resolved.clone(),
            })
            .await;

        self.spawn_transfer_send(resolved, transfer_id.clone());

        Ok((transfer_id, filename, progress))
    }

    pub async fn cancel_transfer(&self, transfer_id: &str) -> Result<(), String> {
        self.cancelled_transfers
            .write()
            .await
            .insert(transfer_id.to_string());

        let (peer_id, filename) = {
            let mut outgoing = self.transfers.write().await;
            if let Some(active) = outgoing.remove(transfer_id) {
                (
                    Some(active.peer_id),
                    Some(active.sender.manifest.filename.clone()),
                )
            } else {
                (None, None)
            }
        };

        self.incoming_transfers.write().await.remove(transfer_id);

        if let (Some(peer_id), Some(filename)) = (peer_id, filename) {
            let msg = ChatMessage {
                id: uuid::Uuid::new_v4(),
                sender_id: self.public_key_hex(),
                recipient_id: peer_id.clone(),
                msg_type: MessageType::File,
                content: String::new(),
                timestamp: chrono::Utc::now(),
                metadata: Some(serde_json::json!({
                    "transfer_id": transfer_id,
                    "action": "cancel",
                })),
            };
            let _ = self.send_wire_message(&peer_id, msg).await;
            let _ = self
                .event_tx
                .send(EngineEvent::TransferCancelled {
                    id: transfer_id.to_string(),
                    filename,
                    peer_id,
                })
                .await;
        }
        Ok(())
    }

    fn spawn_transfer_send(&self, peer_id: String, transfer_id: String) {
        let inbound = self.clone_for_inbound();
        tokio::spawn(async move {
            inbound.run_transfer_send(peer_id, transfer_id).await;
        });
    }

    /// Relay WebRTC signaling (offer/answer/ice/end) over the encrypted channel.
    pub async fn send_call_signal(
        &self,
        peer_id: &str,
        call_id: &str,
        signal: &str,
        payload: &str,
        is_video: bool,
    ) -> Result<(), String> {
        let resolved = self.resolve_peer_id(peer_id).await?;
        let msg_type = match signal {
            "offer" => MessageType::CallOffer,
            "answer" => MessageType::CallAnswer,
            "ice" => MessageType::CallIce,
            "end" => MessageType::CallAnswer,
            _ => return Err(format!("unknown call signal: {signal}")),
        };
        if signal == "end" {
            let _ = self
                .event_tx
                .send(EngineEvent::CallEnded {
                    call_id: call_id.to_string(),
                })
                .await;
        }
        let msg = ChatMessage {
            id: uuid::Uuid::new_v4(),
            sender_id: self.public_key_hex(),
            recipient_id: resolved.clone(),
            msg_type,
            content: payload.to_string(),
            timestamp: chrono::Utc::now(),
            metadata: Some(serde_json::json!({
                "call_id": call_id,
                "signal": signal,
                "is_video": is_video,
            })),
        };
        self.send_wire_message(&resolved, msg).await
    }

    /// Deprecated — use `send_call_signal` from platform WebRTC.
    pub async fn start_call(&self, peer_id: &str, is_video: bool) -> Result<String, String> {
        let call_id = uuid::Uuid::new_v4().to_string();
        self.send_call_signal(peer_id, &call_id, "offer", "", is_video)
            .await?;
        Ok(call_id)
    }

    pub async fn end_call(&self, peer_id: &str, call_id: &str) -> Result<(), String> {
        self.send_call_signal(peer_id, call_id, "end", "", false)
            .await?;
        self.calls.write().await.remove(call_id);
        Ok(())
    }

    async fn send_wire_frame(&self, peer_id: &str, frame: &WireFrame) -> Result<(), String> {
        let wire = frame
            .serialize()
            .map_err(|e: crate::crypto::wire::WireError| e.to_string())?;
        self.send_raw(peer_id, &wire).await
    }

    async fn send_wire_message(&self, peer_id: &str, msg: ChatMessage) -> Result<(), String> {
        let resolved = self.resolve_peer_id(peer_id).await?;
        let transport = self
            .peers
            .read()
            .await
            .get(&resolved)
            .map(|s| s.transport)
            .ok_or_else(|| format!("peer not found: {resolved}"))?;
        let connected = match transport {
            TransportKind::Serial => self.serial.read().await.is_some(),
            _ => self.iroh.read().await.has_connection(&resolved).await,
        };
        if !connected {
            return Err("peer not connected — reconnect from Saved Peers".into());
        }
        if !self.is_peer_trusted(&resolved).await {
            return Err("peer not trusted — confirm SAS first".into());
        }
        let plaintext = msg.to_json().map_err(|e| e.to_string())?;

        let ciphertext = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(&resolved)
                .ok_or_else(|| format!("peer not found: {resolved}"))?;
            session.crypto.encrypt(&plaintext)?
        };

        let payload = EncryptedPayload {
            version: 3,
            ciphertext,
        };
        let wire = WireFrame::Encrypted(payload);
        self.send_wire_frame(&resolved, &wire).await
    }

    async fn send_raw(&self, peer_id: &str, wire: &[u8]) -> Result<(), String> {
        let resolved = self.resolve_peer_id(peer_id).await?;
        let transport = self
            .peers
            .read()
            .await
            .get(&resolved)
            .map(|s| s.transport)
            .ok_or_else(|| format!("peer not found: {resolved}"))?;

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
            TransportKind::Lan | TransportKind::Wan | TransportKind::Relay => {
                self.iroh
                    .read()
                    .await
                    .send(&resolved, wire)
                    .await
                    .map_err(|e| e.to_string())?;
                info!(peer = %resolved, len = wire.len(), "sent wire frame");
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

        self.iroh.write().await.shutdown().await;
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
    iroh: Arc<RwLock<IrohTransport>>,
    serial: Arc<RwLock<Option<SerialTransport>>>,
    peers: Arc<RwLock<HashMap<String, PeerSession>>>,
    event_tx: mpsc::Sender<EngineEvent>,
    handshake_wait: Arc<RwLock<HashMap<String, oneshot::Sender<SignedHandshake>>>>,
    peer_aliases: Arc<RwLock<HashMap<String, String>>>,
    trusted_pubkeys: Arc<RwLock<HashSet<String>>>,
    incoming_transfers: Arc<RwLock<HashMap<String, ChunkedReceiver>>>,
    receive_dir: Arc<RwLock<PathBuf>>,
    transfers: Arc<RwLock<HashMap<String, ActiveTransfer>>>,
    pending_outbound: Arc<RwLock<HashMap<String, VecDeque<PendingOutbound>>>>,
    cancelled_transfers: Arc<RwLock<HashSet<String>>>,
    user_paused: Arc<RwLock<HashSet<String>>>,
    local_display_name: Arc<RwLock<String>>,
    remote_display_names: Arc<RwLock<HashMap<String, String>>>,
}

impl EngineInbound {
    async fn send_profile(&self, peer_id: &str) {
        let name = self.local_display_name.read().await.clone();
        if name.is_empty() {
            return;
        }
        let msg = ChatMessage {
            id: uuid::Uuid::new_v4(),
            sender_id: self.identity.public_key_hex(),
            recipient_id: peer_id.to_string(),
            msg_type: MessageType::System,
            content: name,
            timestamp: chrono::Utc::now(),
            metadata: Some(serde_json::json!({ "action": "profile" })),
        };
        let _ = self.send_app_message(peer_id, msg).await;
    }

    async fn flush_pending_inbound(&self, peer_id: &str) {
        let items: Vec<PendingOutbound> = {
            let mut map = self.pending_outbound.write().await;
            map.remove(peer_id)
                .map(|q| q.into_iter().collect())
                .unwrap_or_default()
        };
        for item in items {
            match item {
                PendingOutbound::Text(content) => {
                    let msg =
                        ChatMessage::text(&self.identity.public_key_hex(), peer_id, &content);
                    let _ = self.send_app_message(peer_id, msg).await;
                }
                PendingOutbound::File(_) => {}
            }
        }
    }

    async fn handle_inbound_bytes(&self, peer_id: &str, data: &[u8]) {
        let resolved = match resolve_session_peer(&self.peers, &self.peer_aliases, peer_id).await {
            Ok(id) => id,
            Err(e) => {
                warn!(error = %e, inbound_id = %peer_id, "inbound peer resolution failed");
                peer_id.to_string()
            }
        };
        if let Ok(frame) = WireFrame::deserialize(data) {
            self.handle_wire_frame(&resolved, frame).await;
            return;
        }
        // Try legacy JSON if postcard parse failed on non-magic data
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
            let (resp, kex) = session.crypto.responder_process_step1(&self.identity, &init)?;
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
            session.pending_kex = None;
            session.crypto.responder_process_step3(&self.identity, &finish)
        }
        .await;

        match sas_result {
            Ok(sas) => {
                let canonical = match self.canonicalize_peer_inbound(peer_id).await {
                    Ok(id) => id,
                    Err(_) => peer_id.to_string(),
                };
                let remote_pk = {
                    self.peers
                        .read()
                        .await
                        .get(&canonical)
                        .map(|s| s.crypto.remote_identity)
                        .unwrap_or([0u8; 32])
                };
                let auto_trusted = if remote_pk != [0u8; 32] {
                    let pk_hex = hex::encode(remote_pk).to_lowercase();
                    if self.trusted_pubkeys.read().await.contains(&pk_hex) {
                        if let Some(s) = self.peers.write().await.get_mut(&canonical) {
                            let _ = s.crypto.confirm_trusted();
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if auto_trusted {
                    self.flush_pending_inbound(&canonical).await;
                    if self
                        .peers
                        .read()
                        .await
                        .get(&canonical)
                        .is_some_and(|s| s.crypto.can_send_encrypted())
                    {
                        self.send_profile(&canonical).await;
                    }
                }
                self.user_paused.write().await.remove(&canonical);
                let _ = self
                    .event_tx
                    .send(EngineEvent::SasReady {
                        peer_id: canonical,
                        sas: sas.clone(),
                        auto_trusted,
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

        let became_send_ready = {
            let peers = self.peers.read().await;
            peers
                .get(peer_id)
                .is_some_and(|s| s.crypto.is_trusted() && s.crypto.can_send_encrypted())
        };

        if let Ok(msg) = ChatMessage::from_json(&plaintext) {
            if msg.metadata.as_ref().and_then(|m| m.get("action")).and_then(|v| v.as_str())
                == Some("ratchet_open")
            {
                if became_send_ready {
                    let canonical = self
                        .canonicalize_peer_inbound(peer_id)
                        .await
                        .unwrap_or_else(|_| peer_id.to_string());
                    self.send_profile(&canonical).await;
                    self.flush_pending_inbound(&canonical).await;
                }
                return;
            }
            self.handle_app_message(peer_id, msg).await;
        } else if became_send_ready {
            let canonical = self
                .canonicalize_peer_inbound(peer_id)
                .await
                .unwrap_or_else(|_| peer_id.to_string());
            self.send_profile(&canonical).await;
            self.flush_pending_inbound(&canonical).await;
        }
    }

    async fn handle_app_message(&self, peer_id: &str, msg: ChatMessage) {
        if msg.msg_type == MessageType::File {
            if let Some(meta) = &msg.metadata {
                match meta.get("action").and_then(|v| v.as_str()) {
                    Some("offer") => {
                        let transfer_id = meta
                            .get("transfer_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let filename = meta
                            .get("filename")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&msg.content)
                            .to_string();
                        let total_chunks = meta
                            .get("total_chunks")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;
                        let total_size = meta
                            .get("total_size")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let sha256 = meta
                            .get("sha256")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if let Ok(id) = uuid::Uuid::parse_str(&transfer_id) {
                            let manifest = TransferManifest {
                                id,
                                filename: filename.clone(),
                                total_size,
                                chunk_size: crate::transfer::DEFAULT_CHUNK_SIZE,
                                total_chunks,
                                sha256,
                            };
                            self.incoming_transfers
                                .write()
                                .await
                                .insert(transfer_id.clone(), ChunkedReceiver::new(manifest));
                            let _ = self
                                .event_tx
                                .send(EngineEvent::TransferProgress {
                                    id: transfer_id,
                                    filename,
                                    progress: 0.0,
                                    peer_id: peer_id.to_string(),
                                })
                                .await;
                        }
                        return;
                    }
                    Some("chunk") => {
                        let transfer_id = meta
                            .get("transfer_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let chunk_id = meta
                            .get("chunk_id")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;
                        let data_b64 = meta
                            .get("data_b64")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if let Ok(data) = base64::Engine::decode(
                            &base64::engine::general_purpose::STANDARD,
                            data_b64,
                        ) {
                            let (ack, complete_info) = {
                                let mut incoming = self.incoming_transfers.write().await;
                                let Some(receiver) = incoming.get_mut(transfer_id) else {
                                    return;
                                };
                                let filename = receiver.manifest.filename.clone();
                                let id = receiver.manifest.id.to_string();
                                let _ =
                                    receiver.receive_chunk(chunk_id, bytes::Bytes::from(data));
                                let progress = receiver.progress();
                                let _ = self
                                    .event_tx
                                    .send(EngineEvent::TransferProgress {
                                        id: id.clone(),
                                        filename: filename.clone(),
                                        progress,
                                        peer_id: peer_id.to_string(),
                                    })
                                    .await;
                                let ack = receiver.selective_ack();
                                let done = if receiver.is_complete() {
                                    if let Ok(data) = receiver.assemble() {
                                        let dir = self.receive_dir.read().await.clone();
                                        let _ = std::fs::create_dir_all(&dir);
                                        let storage_name =
                                            P2pEngine::transfer_storage_name(transfer_id, &filename);
                                        let path = dir.join(&storage_name);
                                        if std::fs::write(&path, &data).is_ok() {
                                            Some((id, filename, path.to_string_lossy().to_string()))
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                };
                                if done.is_some() {
                                    incoming.remove(transfer_id);
                                }
                                (ack, done)
                            };
                            let ack_msg = ChatMessage {
                                id: uuid::Uuid::new_v4(),
                                sender_id: self.identity.public_key_hex(),
                                recipient_id: peer_id.to_string(),
                                msg_type: MessageType::File,
                                content: String::new(),
                                timestamp: chrono::Utc::now(),
                                metadata: Some(serde_json::json!({
                                    "transfer_id": transfer_id,
                                    "action": "ack",
                                    "ack": ack,
                                })),
                            };
                            let _ = self.send_app_message(peer_id, ack_msg).await;
                            if let Some((id, filename, path)) = complete_info {
                                let _ = self
                                    .event_tx
                                    .send(EngineEvent::TransferComplete {
                                        id,
                                        filename,
                                        peer_id: peer_id.to_string(),
                                        path,
                                    })
                                    .await;
                            }
                        }
                        return;
                    }
                    Some("ack") => {
                        let tid = meta
                            .get("transfer_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if let Some(ack) = meta.get("ack").and_then(|v| {
                            serde_json::from_value::<ChunkAck>(v.clone()).ok()
                        }) {
                            let mut transfers = self.transfers.write().await;
                            if let Some(active) = transfers.get_mut(tid) {
                                active.sender.on_ack(&ack);
                            }
                        }
                        return;
                    }
                    Some("cancel") => {
                        let tid = meta
                            .get("transfer_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        self.incoming_transfers.write().await.remove(tid);
                        let _ = self
                            .event_tx
                            .send(EngineEvent::TransferCancelled {
                                id: tid.to_string(),
                                filename: String::new(),
                                peer_id: peer_id.to_string(),
                            })
                            .await;
                        return;
                    }
                    _ => {}
                }
            }
        }

        if matches!(
            msg.msg_type,
            MessageType::CallOffer | MessageType::CallAnswer | MessageType::CallIce
        ) {
            if let Some(meta) = &msg.metadata {
                let call_id = meta
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let signal = meta
                    .get("signal")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| match msg.msg_type {
                        MessageType::CallOffer => "offer",
                        MessageType::CallAnswer => "answer",
                        MessageType::CallIce => "ice",
                        _ => "unknown",
                    });
                let is_video = meta
                    .get("is_video")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !call_id.is_empty() {
                    if signal == "end" {
                        let _ = self
                            .event_tx
                            .send(EngineEvent::CallEnded { call_id })
                            .await;
                    } else {
                        let _ = self
                            .event_tx
                            .send(EngineEvent::CallSignaling {
                                call_id,
                                peer_id: peer_id.to_string(),
                                signal: signal.to_string(),
                                payload: msg.content.clone(),
                                is_video,
                            })
                            .await;
                    }
                    return;
                }
            }
        }

        if msg.msg_type == MessageType::System {
            if let Some(meta) = &msg.metadata {
                if meta.get("action").and_then(|v| v.as_str()) == Some("profile") {
                    let name = msg.content.trim().to_string();
                    if !name.is_empty() {
                        self.remote_display_names
                            .write()
                            .await
                            .insert(peer_id.to_string(), name.clone());
                        let _ = self
                            .event_tx
                            .send(EngineEvent::PeerProfile {
                                peer_id: peer_id.to_string(),
                                display_name: name,
                            })
                            .await;
                    }
                    return;
                }
            }
        }

        let _ = self.event_tx.send(EngineEvent::MessageReceived(msg)).await;
    }

    async fn canonicalize_peer_inbound(&self, conn_peer_id: &str) -> Result<String, String> {
        let canonical = {
            let peers = self.peers.read().await;
            let session = peers
                .get(conn_peer_id)
                .ok_or_else(|| format!("peer not found: {conn_peer_id}"))?;
            if session.crypto.remote_identity == [0u8; 32] {
                return Ok(conn_peer_id.to_string());
            }
            peer_id_from_pubkey(&session.crypto.remote_identity)
        };

        if canonical == conn_peer_id {
            return Ok(canonical);
        }

        let session = {
            let mut peers = self.peers.write().await;
            peers
                .remove(conn_peer_id)
                .ok_or_else(|| format!("peer not found: {conn_peer_id}"))?
        };
        self.peers.write().await.insert(canonical.clone(), session);
        self.iroh.read().await.rekey(conn_peer_id, &canonical).await;
        if conn_peer_id != canonical {
            self.peer_aliases
                .write()
                .await
                .insert(conn_peer_id.to_string(), canonical.clone());
        }

        if let Some(tx) = self.handshake_wait.write().await.remove(conn_peer_id) {
            self.handshake_wait
                .write()
                .await
                .insert(canonical.clone(), tx);
        }

        let _ = self
            .event_tx
            .send(EngineEvent::PeerIdUpdated {
                old_id: conn_peer_id.to_string(),
                new_id: canonical.clone(),
            })
            .await;

        Ok(canonical)
    }

    async fn run_transfer_send(&self, peer_id: String, transfer_id: String) {
        loop {
            if self.cancelled_transfers.read().await.contains(&transfer_id) {
                self.transfers.write().await.remove(&transfer_id);
                return;
            }
            let batch = {
                let mut transfers = self.transfers.write().await;
                let Some(active) = transfers.get_mut(&transfer_id) else {
                    break;
                };
                if active.sender.is_complete() {
                    let filename = active.sender.manifest.filename.clone();
                    transfers.remove(&transfer_id);
                    let _ = self
                        .event_tx
                        .send(EngineEvent::TransferComplete {
                            id: transfer_id.clone(),
                            filename,
                            peer_id: peer_id.clone(),
                            path: String::new(),
                        })
                        .await;
                    return;
                }
                let chunks = active.sender.next_chunks(8);
                let filename = active.sender.manifest.filename.clone();
                let progress = active.sender.progress();
                let _ = self
                    .event_tx
                    .send(EngineEvent::TransferProgress {
                        id: transfer_id.clone(),
                        filename: filename.clone(),
                        progress,
                        peer_id: peer_id.clone(),
                    })
                    .await;
                chunks
            };

            if batch.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }

            for (chunk_id, data) in batch {
                if self.cancelled_transfers.read().await.contains(&transfer_id) {
                    return;
                }
                let msg = ChatMessage {
                    id: uuid::Uuid::new_v4(),
                    sender_id: self.identity.public_key_hex(),
                    recipient_id: peer_id.clone(),
                    msg_type: MessageType::File,
                    content: String::new(),
                    timestamp: chrono::Utc::now(),
                    metadata: Some(serde_json::json!({
                        "transfer_id": transfer_id,
                        "action": "chunk",
                        "chunk_id": chunk_id,
                        "data_b64": base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &data,
                        ),
                    })),
                };
                if self.send_app_message(&peer_id, msg).await.is_err() {
                    warn!(transfer_id = %transfer_id, "chunk send failed — will retry");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn send_app_message(&self, peer_id: &str, msg: ChatMessage) -> Result<(), String> {
        if !self.iroh.read().await.has_connection(peer_id).await {
            return Err("peer not connected".into());
        }
        let plaintext = msg.to_json().map_err(|e| e.to_string())?;
        let ciphertext = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(peer_id)
                .ok_or_else(|| format!("peer not found: {peer_id}"))?;
            if !session.crypto.is_trusted() {
                return Err("peer not trusted — confirm SAS first".into());
            }
            session.crypto.encrypt(&plaintext)?
        };
        let payload = EncryptedPayload {
            version: 3,
            ciphertext,
        };
        self.send_raw_frame(peer_id, &WireFrame::Encrypted(payload))
            .await
    }

    async fn send_raw_frame(&self, peer_id: &str, frame: &WireFrame) -> Result<(), String> {
        let wire = frame
            .serialize()
            .map_err(|e: crate::crypto::wire::WireError| e.to_string())?;
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
            TransportKind::Lan | TransportKind::Wan | TransportKind::Relay => {
                self.iroh
                    .read()
                    .await
                    .send(peer_id, &wire)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

