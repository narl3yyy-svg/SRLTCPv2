//! Central P2P engine coordinating transports, crypto sessions, and messaging.
//!
//! `P2pEngine` is a cheap, cloneable handle around shared state; every background task
//! (accept loop, per-connection read loops, transfers, reconnects) holds a clone. All
//! methods take `&self`, so callers do not need an outer mutex.
//!
//! Peer ids:
//! - `iroh:{endpoint id}` / `serial:{port}` — a transport connection before its handshake.
//! - `peer:{ed25519 hex}` — the canonical id once the signed handshake proved the identity.
//!   Transport ids stay registered as aliases of the canonical id.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::Connection;
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{debug, info, warn};

use crate::crypto::handshake::HybridKeyExchange;
use crate::crypto::identity::{normalize_qr_input, parse_qr_payload, Identity, ParsedQr};
use crate::crypto::peer_crypto::PeerCrypto;
use crate::crypto::wire::{EncryptedPayload, SignedHandshake, WireFrame};
use crate::network::iroh_transport::RegisterOutcome;
use crate::network::{IrohTransport, TransportKind};
use crate::protocol::{ChatMessage, MessageType};
use crate::serial::{list_ports, SerialConfig, SerialEvent, SerialTransport};
use crate::transfer::{
    sanitize_filename, ChunkAck, ChunkedReceiver, ChunkedSender, TransferManifest,
    DEFAULT_CHUNK_SIZE, IROH_CHUNK_SIZE,
};
use crate::webrtc::CallSession;

fn peer_id_from_pubkey(pk: &[u8; 32]) -> String {
    format!("peer:{}", hex::encode(pk))
}

fn iroh_peer_id(node_id: impl std::fmt::Display) -> String {
    format!("iroh:{node_id}")
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
        total_bytes: u64,
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
    /// Transfer aborted by an error (hash mismatch, disk write, peer lost…).
    TransferFailed {
        id: String,
        filename: String,
        peer_id: String,
        reason: String,
    },
    CallSignaling {
        call_id: String,
        peer_id: String,
        signal: String,
        payload: String,
        is_video: bool,
    },
    CallEnded {
        call_id: String,
        peer_id: Option<String>,
    },
    PeerQrRefresh {
        peer_id: String,
        qr: String,
    },
    MessageQueued { peer_id: String, queue_size: usize },
    Reconnecting { peer_id: String },
    PeerProfile { peer_id: String, display_name: String },
    Error(String),
}

const MAX_PENDING_PER_PEER: usize = 500;
/// Messages from a peer that has not passed SAS yet are held (not shown) until confirmed.
const MAX_HELD_INBOUND: usize = 256;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Returned when both sides started a handshake at once and the peer's one won.
const SUPERSEDED: &str = "handshake superseded by the peer's simultaneous handshake";
/// Give up on an outgoing transfer when no new chunk is acknowledged for this long.
const TRANSFER_STALL: Duration = Duration::from_secs(90);

#[derive(Debug, Clone)]
enum PendingOutbound {
    Text(String),
    File(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandshakeRole {
    Initiator,
    Responder,
}

enum HandshakeSignal {
    Step2(SignedHandshake),
    /// The peer started its own handshake and won the tie-break; we became responder.
    Superseded,
}

/// (canonical peer id, SAS, auto-trusted)
type HandshakeOutcome = Result<(String, String, bool), String>;

struct PeerSession {
    transport: TransportKind,
    crypto: PeerCrypto,
    /// In-flight hybrid KEX state.
    pending_kex: Option<HybridKeyExchange>,
    /// Our role while a handshake is in progress on this session.
    role: Option<HandshakeRole>,
    /// Signed step-1 frame, kept so it can be resent if the connection is swapped mid-handshake.
    step1: Option<SignedHandshake>,
    /// Signature of the peer's step 1 we answered, and our answer. A resent duplicate of that
    /// step 1 gets the same answer instead of a fresh key exchange (which would desync keys).
    answered_step1: Option<Vec<u8>>,
    step2: Option<SignedHandshake>,
}

impl PeerSession {
    fn new(transport: TransportKind) -> Self {
        Self {
            transport,
            crypto: PeerCrypto::new_connected(),
            pending_kex: None,
            role: None,
            step1: None,
            answered_step1: None,
            step2: None,
        }
    }
}

struct ActiveTransfer {
    sender: ChunkedSender,
    peer_id: String,
}

struct IncomingTransfer {
    receiver: ChunkedReceiver,
    peer_id: String,
}

struct ActiveCall {
    #[allow(dead_code)]
    session: CallSession,
}

/// Shared engine state (reached through [`P2pEngine`]'s `Deref`).
#[doc(hidden)]
pub struct Shared {
    identity: Identity,
    running: RwLock<bool>,
    iroh: RwLock<IrohTransport>,
    serial: RwLock<Option<Arc<SerialTransport>>>,
    peers: RwLock<HashMap<String, PeerSession>>,
    transfers: RwLock<HashMap<String, ActiveTransfer>>,
    incoming_transfers: RwLock<HashMap<String, IncomingTransfer>>,
    /// Finished incoming transfers, so late duplicate chunks are still acknowledged.
    completed_incoming: RwLock<HashSet<String>>,
    calls: RwLock<HashMap<String, ActiveCall>>,
    /// One active call per canonical peer id (peer:{pubkey}).
    active_peer_calls: RwLock<HashMap<String, String>>,
    event_tx: mpsc::Sender<EngineEvent>,
    /// Initiator step-2 waiters keyed by session id.
    handshake_wait: RwLock<HashMap<String, oneshot::Sender<HandshakeSignal>>>,
    /// Callers waiting for any handshake with this identity (pubkey hex) to finish.
    handshake_done: RwLock<HashMap<String, Vec<oneshot::Sender<HandshakeOutcome>>>>,
    /// Identities (pubkey hex) with a dial + handshake in flight.
    dials_in_progress: RwLock<HashSet<String>>,
    reconnecting: RwLock<HashSet<String>>,
    /// Maps transport ids (iroh:…, serial:…) to canonical peer:{pubkey}.
    peer_aliases: RwLock<HashMap<String, String>>,
    /// Previously verified peer Ed25519 public keys (hex).
    trusted_pubkeys: RwLock<HashSet<String>>,
    receive_dir: RwLock<PathBuf>,
    /// Outbound messages/files waiting for reconnect.
    pending_outbound: RwLock<HashMap<String, VecDeque<PendingOutbound>>>,
    /// Inbound messages from peers awaiting SAS confirmation.
    held_inbound: RwLock<HashMap<String, Vec<ChatMessage>>>,
    /// Saved peer QR payloads for seamless auto-reconnect.
    saved_peer_qr: RwLock<HashMap<String, String>>,
    cancelled_transfers: RwLock<HashSet<String>>,
    user_paused: RwLock<HashSet<String>>,
    local_display_name: RwLock<String>,
    remote_display_names: RwLock<HashMap<String, String>>,
}

/// Main P2P engine — cloneable, thread-safe handle.
#[derive(Clone)]
pub struct P2pEngine {
    shared: Arc<Shared>,
}

impl Deref for P2pEngine {
    type Target = Shared;
    fn deref(&self) -> &Shared {
        &self.shared
    }
}

enum TransferStep {
    Send(Vec<(u32, bytes::Bytes)>, String, f64, u64),
    Done(String),
    Fail(String, String),
}

enum ChunkOutcome {
    Ignore,
    Ack(ChunkAck, Option<(String, f64, u64)>),
    Complete(ChunkAck, Box<ChunkedReceiver>),
    Fail(String, String),
}

impl P2pEngine {
    /// Create engine with a fresh ephemeral identity (tests / one-shot use).
    pub fn new() -> (Self, mpsc::Receiver<EngineEvent>) {
        Self::with_identity(Identity::generate())
    }

    /// Create engine with a stable long-term identity (production path).
    pub fn with_identity(identity: Identity) -> (Self, mpsc::Receiver<EngineEvent>) {
        let (event_tx, event_rx) = mpsc::channel(512);
        let shared = Shared {
            identity,
            running: RwLock::new(false),
            iroh: RwLock::new(IrohTransport::new()),
            serial: RwLock::new(None),
            peers: RwLock::new(HashMap::new()),
            transfers: RwLock::new(HashMap::new()),
            incoming_transfers: RwLock::new(HashMap::new()),
            completed_incoming: RwLock::new(HashSet::new()),
            calls: RwLock::new(HashMap::new()),
            active_peer_calls: RwLock::new(HashMap::new()),
            event_tx,
            handshake_wait: RwLock::new(HashMap::new()),
            handshake_done: RwLock::new(HashMap::new()),
            dials_in_progress: RwLock::new(HashSet::new()),
            reconnecting: RwLock::new(HashSet::new()),
            peer_aliases: RwLock::new(HashMap::new()),
            trusted_pubkeys: RwLock::new(HashSet::new()),
            receive_dir: RwLock::new(std::env::temp_dir().join("srltcp_received")),
            pending_outbound: RwLock::new(HashMap::new()),
            held_inbound: RwLock::new(HashMap::new()),
            saved_peer_qr: RwLock::new(HashMap::new()),
            cancelled_transfers: RwLock::new(HashSet::new()),
            user_paused: RwLock::new(HashSet::new()),
            local_display_name: RwLock::new(String::new()),
            remote_display_names: RwLock::new(HashMap::new()),
        };
        (
            Self {
                shared: Arc::new(shared),
            },
            event_rx,
        )
    }

    async fn emit(&self, event: EngineEvent) {
        let _ = self.event_tx.send(event).await;
    }

    /// High-frequency events (progress) are dropped rather than stalling the engine when the
    /// UI is slow to drain the channel.
    fn emit_lossy(&self, event: EngineEvent) {
        let _ = self.event_tx.try_send(event);
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

    /// Resolve transport/alias ids to the session key in `peers`.
    async fn resolve_peer_id(&self, peer_id: &str) -> Result<String, String> {
        if let Some(canonical) = self.peer_aliases.read().await.get(peer_id) {
            if self.peers.read().await.contains_key(canonical) {
                return Ok(canonical.clone());
            }
        }
        let peers = self.peers.read().await;
        if peers.contains_key(peer_id) {
            return Ok(peer_id.to_string());
        }
        if let Some(hex_id) = peer_id.strip_prefix("peer:") {
            let hex_id = hex_id.to_lowercase();
            for (id, session) in peers.iter() {
                if hex::encode(session.crypto.remote_identity) == hex_id {
                    return Ok(id.clone());
                }
            }
        }
        Err(format!("peer not connected: {peer_id}"))
    }

    fn pubkey_hex_from_peer_id(peer_id: &str) -> Option<String> {
        peer_id
            .strip_prefix("peer:")
            .map(|h| h.to_lowercase())
            .filter(|h| !h.is_empty())
    }

    pub async fn register_saved_peer(&self, peer_id: &str, qr: &str) {
        let key = match self.resolve_peer_id(peer_id).await {
            Ok(canonical) if canonical.starts_with("peer:") => canonical,
            _ if peer_id.starts_with("peer:") => peer_id.to_string(),
            _ => return,
        };
        self.saved_peer_qr
            .write()
            .await
            .insert(key, qr.trim().to_string());
    }

    async fn can_queue_for_peer(&self, peer_id: &str) -> bool {
        match Self::pubkey_hex_from_peer_id(peer_id) {
            Some(hex) => self.trusted_pubkeys.read().await.contains(&hex),
            None => false,
        }
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
        let items: Vec<PendingOutbound> = self
            .pending_outbound
            .write()
            .await
            .remove(peer_id)
            .map(|q| q.into_iter().collect())
            .unwrap_or_default();
        let mut retry = Vec::new();
        for item in items {
            let res = match &item {
                PendingOutbound::Text(content) => self.try_send_message_now(peer_id, content).await,
                PendingOutbound::File(path) => self
                    .try_send_file_now(peer_id, &path.to_string_lossy())
                    .await
                    .map(|_| ()),
            };
            if let Err(e) = res {
                // Keep it queued if the link is not usable yet; drop on permanent errors
                // (e.g. file deleted) so one bad entry does not block the queue forever.
                if e.contains("ratchet send chain not ready") || e.contains("not connected") || e.contains("not trusted") {
                    retry.push(item);
                } else {
                    warn!(error = %e, "dropping queued item");
                    self.emit(EngineEvent::Error(format!("queued item not sent: {e}"))).await;
                }
            }
        }
        if !retry.is_empty() {
            let mut map = self.pending_outbound.write().await;
            let q = map.entry(peer_id.to_string()).or_default();
            for (i, item) in retry.into_iter().enumerate() {
                q.insert(i, item);
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
        let msg = self.system_message(&resolved, "", "ratchet_open");
        self.send_wire_message(&resolved, msg).await
    }

    fn system_message(&self, to: &str, content: &str, action: &str) -> ChatMessage {
        ChatMessage {
            id: uuid::Uuid::new_v4(),
            sender_id: self.public_key_hex(),
            recipient_id: to.to_string(),
            msg_type: MessageType::System,
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
            metadata: Some(serde_json::json!({ "action": action })),
        }
    }

    fn file_message(&self, to: &str, meta: serde_json::Value) -> ChatMessage {
        ChatMessage {
            id: uuid::Uuid::new_v4(),
            sender_id: self.public_key_hex(),
            recipient_id: to.to_string(),
            msg_type: MessageType::File,
            content: String::new(),
            timestamp: chrono::Utc::now(),
            metadata: Some(meta),
        }
    }

    fn schedule_auto_reconnect(&self, peer_id: String) {
        let engine = self.clone();
        tokio::spawn(async move {
            if !engine.reconnecting.write().await.insert(peer_id.clone()) {
                return;
            }
            engine.auto_reconnect(&peer_id).await;
            engine.reconnecting.write().await.remove(&peer_id);
        });
    }

    async fn auto_reconnect(&self, peer_id: &str) {
        if self.saved_peer_qr.read().await.get(peer_id).is_none() || !self.can_queue_for_peer(peer_id).await {
            return;
        }
        // Stagger the two sides so they rarely dial each other at the same instant.
        let remote_hex = Self::pubkey_hex_from_peer_id(peer_id).unwrap_or_default();
        let stagger = if self.public_key_hex() > remote_hex { 3 } else { 0 };
        for attempt in 0..6u32 {
            tokio::time::sleep(Duration::from_secs(2u64.pow(attempt.min(4)) + stagger)).await;
            if !*self.running.read().await || self.user_paused.read().await.contains(peer_id) {
                return;
            }
            if self.is_peer_connected(peer_id).await && self.is_peer_trusted(peer_id).await {
                return; // the peer reconnected to us
            }
            let Some(qr) = self.saved_peer_qr.read().await.get(peer_id).cloned() else {
                return;
            };
            self.emit(EngineEvent::Reconnecting { peer_id: peer_id.to_string() }).await;
            match self.connect_and_verify(&qr).await {
                Ok(_) => return,
                Err(e) => debug!(peer = %peer_id, attempt, error = %e, "auto-reconnect attempt failed"),
            }
        }
        if !self.is_peer_connected(peer_id).await {
            self.emit(EngineEvent::PeerDisconnected {
                peer_id: peer_id.to_string(),
                reason: "reconnect failed".into(),
            })
            .await;
        }
    }

    pub async fn set_display_name(&self, name: &str) {
        *self.local_display_name.write().await = name.trim().to_string();
    }

    pub async fn get_display_name(&self, peer_id: &str) -> Option<String> {
        let key = self
            .resolve_peer_id(peer_id)
            .await
            .unwrap_or_else(|_| peer_id.to_string());
        self.remote_display_names.read().await.get(&key).cloned()
    }

    pub async fn broadcast_profile(&self, peer_id: &str) -> Result<(), String> {
        let name = self.local_display_name.read().await.clone();
        if name.is_empty() {
            return Ok(());
        }
        let resolved = self.resolve_peer_id(peer_id).await?;
        if !self.can_send_now(&resolved).await {
            return Ok(());
        }
        let msg = self.system_message(&resolved, &name, "profile");
        self.send_wire_message(&resolved, msg).await
    }

    async fn can_send_now(&self, resolved: &str) -> bool {
        self.peers
            .read()
            .await
            .get(resolved)
            .is_some_and(|s| s.crypto.is_trusted() && s.crypto.can_send_encrypted())
    }

    /// Current v4 QR without waiting for the endpoint (None before it is bound).
    async fn fresh_qr_now(&self) -> Option<String> {
        let ticket = self.iroh.read().await.ticket_string().ok()?;
        Some(self.identity.qr_payload_v4(&ticket))
    }

    /// Share a fresh v4 QR (live iroh ticket) so saved contacts stay reconnectable.
    pub async fn broadcast_fresh_qr(&self, peer_id: &str) -> Result<(), String> {
        let resolved = self.resolve_peer_id(peer_id).await?;
        if !self.can_send_now(&resolved).await {
            return Ok(());
        }
        let Some(qr) = self.fresh_qr_now().await else {
            return Ok(());
        };
        let msg = self.system_message(&resolved, &qr, "qr_refresh");
        self.send_wire_message(&resolved, msg).await
    }

    async fn clear_call_for_peer(&self, peer_id: &str, call_id: &str) {
        let resolved = self
            .resolve_peer_id(peer_id)
            .await
            .unwrap_or_else(|_| peer_id.to_string());
        {
            let mut peer_calls = self.active_peer_calls.write().await;
            if peer_calls.get(&resolved).is_some_and(|id| id == call_id) {
                peer_calls.remove(&resolved);
            }
        }
        self.calls.write().await.remove(call_id);
        self.emit(EngineEvent::CallEnded {
            call_id: call_id.to_string(),
            peer_id: Some(resolved),
        })
        .await;
    }

    fn transfer_storage_name(transfer_id: &str, filename: &str) -> String {
        let prefix = transfer_id.get(..8).unwrap_or(transfer_id);
        format!("{prefix}_{}", sanitize_filename(filename))
    }

    /// Move a session from its transport id to `peer:{pubkey}` once the handshake proved
    /// the identity. Any older session for the same identity is replaced.
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
        {
            let mut peers = self.peers.write().await;
            let session = peers
                .remove(conn_peer_id)
                .ok_or_else(|| format!("peer not found: {conn_peer_id}"))?;
            peers.insert(canonical.clone(), session);
        }
        self.iroh.read().await.rekey(conn_peer_id, &canonical).await;
        self.peer_aliases
            .write()
            .await
            .insert(conn_peer_id.to_string(), canonical.clone());
        let waiter = self.handshake_wait.write().await.remove(conn_peer_id);
        if let Some(tx) = waiter {
            self.handshake_wait.write().await.insert(canonical.clone(), tx);
        }
        self.emit(EngineEvent::PeerIdUpdated {
            old_id: conn_peer_id.to_string(),
            new_id: canonical.clone(),
        })
        .await;
        Ok(canonical)
    }

    /// Drop stale sessions for an identity before redialing (keeps in-progress handshakes).
    async fn cleanup_sessions_for_pubkey(&self, pubkey: &[u8; 32]) {
        let canonical = peer_id_from_pubkey(pubkey);
        let stale: Vec<String> = self
            .peers
            .read()
            .await
            .iter()
            .filter(|(id, s)| (*id == &canonical || s.crypto.remote_identity == *pubkey) && s.role.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            self.teardown_peer_session(&id, false).await;
        }
    }

    /// Drop transport + session. `user_initiated` marks the peer paused (no auto-reconnect).
    async fn teardown_peer_session(&self, peer_id: &str, user_initiated: bool) {
        let resolved = self
            .resolve_peer_id(peer_id)
            .await
            .unwrap_or_else(|_| peer_id.to_string());
        self.cancel_peer_transfers(&resolved).await;

        let transport = self.peers.read().await.get(&resolved).map(|s| s.transport);
        if transport == Some(TransportKind::Serial) {
            let serial = self.serial.write().await.take();
            if let Some(t) = serial {
                t.shutdown().await;
            }
        } else {
            let iroh = self.iroh.read().await;
            iroh.unregister(&resolved).await;
            if peer_id != resolved {
                iroh.unregister(peer_id).await;
            }
        }

        {
            let mut peers = self.peers.write().await;
            peers.remove(&resolved);
            peers.remove(peer_id);
        }
        {
            let mut waits = self.handshake_wait.write().await;
            waits.remove(&resolved);
            waits.remove(peer_id);
        }
        self.held_inbound.write().await.remove(&resolved);

        if user_initiated {
            self.user_paused.write().await.insert(resolved.clone());
            self.emit(EngineEvent::PeerDisconnected {
                peer_id: resolved,
                reason: "user disconnected".to_string(),
            })
            .await;
        }
    }

    async fn ensure_started(&self) -> Result<(), String> {
        if *self.running.read().await {
            return Ok(());
        }
        self.start(crate::DEFAULT_QUIC_PORT).await
    }

    /// Block until the iroh endpoint is bound and online (for QR v4 generation).
    pub async fn wait_until_ready(&self, timeout_secs: u64) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
        if !*self.running.read().await {
            tokio::time::timeout_at(deadline, self.start(crate::DEFAULT_QUIC_PORT))
                .await
                .map_err(|_| "engine start timed out — check network and retry".to_string())??;
        }
        while tokio::time::Instant::now() < deadline {
            if self.iroh.read().await.is_online() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("iroh endpoint not ready — check network connection and retry".into())
    }

    pub async fn is_ready(&self) -> bool {
        *self.running.read().await && self.iroh.read().await.is_online()
    }

    async fn ensure_connected(&self, parsed: &ParsedQr) -> Result<String, String> {
        self.ensure_started().await?;
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

    /// True when the peer's current transport link is up.
    pub async fn is_peer_connected(&self, peer_id: &str) -> bool {
        match self.resolve_peer_id(peer_id).await {
            Ok(resolved) => self.link_up(&resolved).await,
            Err(_) => false,
        }
    }

    async fn link_up(&self, resolved: &str) -> bool {
        let transport = self.peers.read().await.get(resolved).map(|s| s.transport);
        match transport {
            Some(TransportKind::Serial) => self
                .serial
                .read()
                .await
                .as_ref()
                .is_some_and(|s| s.is_running()),
            Some(_) => self.iroh.read().await.has_connection(resolved).await,
            None => false,
        }
    }

    async fn maybe_auto_trust(&self, peer_id: &str, remote_pk: &[u8; 32]) -> bool {
        let pk_hex = hex::encode(remote_pk);
        if !self.trusted_pubkeys.read().await.contains(&pk_hex) {
            return false;
        }
        self.confirm_peer_trusted(peer_id).await.is_ok()
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
        let endpoint = {
            let mut running = self.running.write().await;
            if *running {
                return Ok(());
            }
            let ep = self
                .iroh
                .write()
                .await
                .bind()
                .await
                .map_err(|e| e.to_string())?;
            *running = true;
            ep
        };
        self.spawn_iroh_accept_loop(endpoint.clone());
        // Wait for relay/direct addresses without holding any lock, so serial and the UI keep
        // working while the network comes up.
        IrohTransport::wait_online(&endpoint).await;
        self.iroh.write().await.mark_online();
        info!("P2P engine started (iroh NAT traversal)");
        self.emit(EngineEvent::Started).await;
        Ok(())
    }

    fn spawn_iroh_accept_loop(&self, endpoint: iroh::Endpoint) {
        let engine = self.clone();
        tokio::spawn(async move {
            // `accept()` yields None once the endpoint is closed (shutdown).
            while let Some(incoming) = endpoint.accept().await {
                let engine = engine.clone();
                tokio::spawn(async move {
                    match incoming.await {
                        Ok(conn) => engine.on_inbound_connection(conn).await,
                        Err(e) => debug!(error = %e, "incoming iroh connection failed"),
                    }
                });
            }
            debug!("iroh accept loop ended");
        });
    }

    async fn on_inbound_connection(&self, conn: Connection) {
        let remote = conn.remote_id();
        let peer_id = iroh_peer_id(remote);
        info!(%remote, "iroh inbound connection accepted");
        let outcome = self
            .iroh
            .read()
            .await
            .register(peer_id.clone(), conn.clone(), false)
            .await;
        if outcome == RegisterOutcome::Rejected {
            debug!(peer = %peer_id, "inbound connection lost dial glare tie-break");
            return;
        }
        self.spawn_iroh_read_loop(peer_id.clone(), conn);
        let resend = {
            let mut peers = self.peers.write().await;
            let session = peers
                .entry(peer_id.clone())
                .or_insert_with(|| PeerSession::new(TransportKind::Lan));
            session.transport = TransportKind::Lan;
            // The connection our step 1 went over may be the one just replaced or closed by
            // dial-glare resolution; resend it on the connection that is current now.
            if session.role == Some(HandshakeRole::Initiator) {
                session.step1.clone()
            } else {
                None
            }
        };
        if let Some(step1) = resend {
            let _ = self.send_raw_frame(&peer_id, &WireFrame::Handshake(step1)).await;
        }
        self.emit(EngineEvent::PeerConnected {
            peer_id,
            transport: TransportKind::Lan,
        })
        .await;
    }

    fn spawn_iroh_read_loop(&self, peer_id: String, conn: Connection) {
        let engine = self.clone();
        let stable_id = conn.stable_id();
        let watched = conn.clone();
        tokio::spawn(async move {
            let reader = engine.clone();
            crate::network::iroh_transport::read_connection_loop(conn, peer_id.clone(), move |pid, data| {
                let reader = reader.clone();
                async move {
                    reader.handle_inbound_bytes(&pid, &data).await;
                }
            })
            .await;
            // The connection may have been re-keyed to the canonical id meanwhile. Only the
            // *current* connection's end means the peer is gone; a replaced one is ignored.
            let resolved = engine
                .resolve_peer_id(&peer_id)
                .await
                .unwrap_or_else(|_| peer_id.clone());
            let current = {
                let iroh = engine.iroh.read().await;
                iroh.connection_id(&resolved).await == Some(stable_id)
                    || iroh.connection_id(&peer_id).await == Some(stable_id)
            };
            if !current {
                debug!(peer = %peer_id, "replaced connection closed");
                return;
            }
            // The peer closed this one because both sides dialed at once (or it reconnected);
            // the winning connection is already up on its side and will register here shortly.
            let superseded = watched.close_reason().is_some_and(|r| {
                let r = r.to_string();
                r.contains("by peer: replaced") || r.contains("by peer: glare")
            });
            if superseded {
                engine.iroh.read().await.unregister_if_current(&resolved, stable_id).await;
                engine.iroh.read().await.unregister_if_current(&peer_id, stable_id).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
                if engine.iroh.read().await.has_connection(&resolved).await
                    || engine.iroh.read().await.has_connection(&peer_id).await
                {
                    return;
                }
            }
            engine.on_connection_lost(&resolved, &peer_id, stable_id).await;
        });
    }

    pub async fn connect_iroh_ticket(&self, ticket: &str) -> Result<String, String> {
        let addr = IrohTransport::parse_ticket(ticket).map_err(|e| format!("invalid iroh ticket: {e}"))?;
        self.connect_iroh_addr(addr, TransportKind::Relay).await
    }

    async fn connect_iroh_addr(&self, addr: iroh::EndpointAddr, kind: TransportKind) -> Result<String, String> {
        let peer_id = iroh_peer_id(addr.id);

        // Reuse a live connection (e.g. the peer just dialed us).
        if self.iroh.read().await.has_connection(&peer_id).await {
            self.peers
                .write()
                .await
                .entry(peer_id.clone())
                .or_insert_with(|| PeerSession::new(kind));
            return Ok(peer_id);
        }

        let endpoint = self
            .iroh
            .read()
            .await
            .endpoint()
            .ok_or_else(|| "iroh endpoint not running".to_string())?;
        let conn = IrohTransport::dial(&endpoint, addr).await.map_err(|e| {
            format!(
                "iroh dial failed ({e}). Peer may be offline, behind a strict firewall, \
                 or using an expired QR — ask them to share a fresh QR while online."
            )
        })?;

        let outcome = self
            .iroh
            .read()
            .await
            .register(peer_id.clone(), conn.clone(), true)
            .await;
        if outcome != RegisterOutcome::Rejected {
            self.spawn_iroh_read_loop(peer_id.clone(), conn);
        }
        self.peers
            .write()
            .await
            .entry(peer_id.clone())
            .or_insert_with(|| PeerSession::new(kind));
        self.emit(EngineEvent::PeerConnected {
            peer_id: peer_id.clone(),
            transport: kind,
        })
        .await;
        Ok(peer_id)
    }

    /// Open a serial port. The far side becomes peer `serial:{port}` until a handshake
    /// canonicalizes it. Idempotent for the port that is already open.
    pub async fn connect_serial(&self, port_name: &str, baud_rate: u32) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            let _ = (port_name, baud_rate);
            return Err("serial transport is not available on Android".to_string());
        }

        #[cfg(not(target_os = "android"))]
        {
            let existing = self.serial.read().await.clone();
            if let Some(t) = existing {
                if t.is_running() && t.port_name() == port_name {
                    return Ok(());
                }
                t.shutdown().await;
            }

            let config = SerialConfig {
                port_name: port_name.to_string(),
                baud_rate,
                ..Default::default()
            };
            let (transport, mut event_rx) = SerialTransport::new(config);
            let transport = Arc::new(transport);
            transport.start().await?;
            *self.serial.write().await = Some(transport.clone());

            let engine = self.clone();
            tokio::spawn(async move {
                let peer_id = format!("serial:{}", transport.port_name());
                while let Some(event) = event_rx.recv().await {
                    match event {
                        SerialEvent::Connected { .. } => {
                            engine
                                .peers
                                .write()
                                .await
                                .insert(peer_id.clone(), PeerSession::new(TransportKind::Serial));
                            engine
                                .emit(EngineEvent::PeerConnected {
                                    peer_id: peer_id.clone(),
                                    transport: TransportKind::Serial,
                                })
                                .await;
                        }
                        SerialEvent::Disconnected { reason, .. } => {
                            let resolved = engine
                                .resolve_peer_id(&peer_id)
                                .await
                                .unwrap_or_else(|_| peer_id.clone());
                            let is_serial = engine
                                .peers
                                .read()
                                .await
                                .get(&resolved)
                                .is_some_and(|s| s.transport == TransportKind::Serial);
                            if is_serial {
                                engine.cancel_peer_transfers(&resolved).await;
                                engine.peers.write().await.remove(&resolved);
                                engine.held_inbound.write().await.remove(&resolved);
                            }
                            engine.peers.write().await.remove(&peer_id);
                            {
                                let mut serial = engine.serial.write().await;
                                if serial.as_ref().is_some_and(|s| Arc::ptr_eq(s, &transport)) {
                                    *serial = None;
                                }
                            }
                            engine
                                .emit(EngineEvent::PeerDisconnected { peer_id: resolved, reason })
                                .await;
                            break;
                        }
                        SerialEvent::DataReceived(data) => {
                            engine.handle_inbound_bytes(&peer_id, &data).await;
                        }
                        SerialEvent::Error(e) => {
                            warn!(error = %e, "serial error");
                            engine.emit(EngineEvent::Error(e)).await;
                        }
                    }
                }
            });
            Ok(())
        }
    }

    /// Open a serial port and run the verified handshake against the peer's QR identity.
    /// Returns (peer_id, sas, auto_trusted) like [`Self::connect_and_verify`].
    pub async fn connect_serial_and_verify(
        &self,
        port_name: &str,
        baud_rate: u32,
        remote_qr: &str,
    ) -> Result<(String, String, bool), String> {
        let parsed = parse_qr_payload(&normalize_qr_input(remote_qr))
            .map_err(|e| format!("invalid peer QR: {e}"))?;
        if parsed.public_key == self.identity.public_key_bytes() {
            return Err("that is your own QR code".into());
        }
        self.connect_serial(port_name, baud_rate).await?;
        let serial_id = format!("serial:{port_name}");
        let mut session_id = None;
        for _ in 0..50 {
            if let Ok(id) = self.resolve_peer_id(&serial_id).await {
                session_id = Some(id);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let session_id = session_id.ok_or_else(|| "serial link did not come up".to_string())?;
        let pk_hex = hex::encode(parsed.public_key);
        let rx = self.register_handshake_waiter(&pk_hex).await;
        match self.handshake_with_qr_bytes(&session_id, &parsed.public_key).await {
            Err(e) if e == SUPERSEDED => self.await_handshake(rx).await,
            other => other,
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

    async fn cancel_peer_transfers(&self, peer_id: &str) {
        let outgoing: Vec<(String, String)> = {
            let mut transfers = self.transfers.write().await;
            let ids: Vec<String> = transfers
                .iter()
                .filter(|(_, t)| t.peer_id == peer_id)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| transfers.remove(&id).map(|t| (id, t.sender.manifest.filename)))
                .collect()
        };
        let incoming: Vec<(String, IncomingTransfer)> = {
            let mut map = self.incoming_transfers.write().await;
            let ids: Vec<String> = map
                .iter()
                .filter(|(_, t)| t.peer_id == peer_id)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| map.remove(&id).map(|t| (id, t)))
                .collect()
        };
        for (id, filename) in outgoing {
            self.cancelled_transfers.write().await.insert(id.clone());
            self.emit(EngineEvent::TransferCancelled {
                id,
                filename,
                peer_id: peer_id.to_string(),
            })
            .await;
        }
        for (id, t) in incoming {
            let filename = t.receiver.manifest.filename.clone();
            t.receiver.discard();
            self.emit(EngineEvent::TransferCancelled {
                id,
                filename,
                peer_id: peer_id.to_string(),
            })
            .await;
        }
    }

    pub async fn disconnect_peer(&self, peer_id: &str) -> Result<(), String> {
        self.teardown_peer_session(peer_id, true).await;
        Ok(())
    }

    async fn on_connection_lost(&self, peer_id: &str, conn_peer_id: &str, stable_id: usize) {
        {
            let iroh = self.iroh.read().await;
            iroh.unregister_if_current(peer_id, stable_id).await;
            iroh.unregister_if_current(conn_peer_id, stable_id).await;
        }
        if self.user_paused.read().await.contains(peer_id) {
            return;
        }
        let was_trusted = {
            let mut peers = self.peers.write().await;
            let trusted = peers.get(peer_id).is_some_and(|s| s.crypto.is_trusted());
            peers.remove(peer_id);
            peers.remove(conn_peer_id);
            trusted
        };
        {
            // Dropping the waiter fails an in-progress initiator handshake immediately.
            let mut waits = self.handshake_wait.write().await;
            waits.remove(peer_id);
            waits.remove(conn_peer_id);
        }
        self.held_inbound.write().await.remove(peer_id);
        self.cancel_peer_transfers(peer_id).await;
        self.emit(EngineEvent::PeerDisconnected {
            peer_id: peer_id.to_string(),
            reason: "connection lost".to_string(),
        })
        .await;
        if was_trusted {
            self.schedule_auto_reconnect(peer_id.to_string());
        }
    }

    /// Connect, run handshake, canonicalize peer id. Returns (peer_id, sas, auto_trusted).
    ///
    /// Safe to call concurrently for the same peer (UI click + auto-reconnect) and when the
    /// peer dials us at the same moment: everyone shares the one handshake that wins.
    pub async fn connect_and_verify(&self, remote_qr: &str) -> Result<(String, String, bool), String> {
        let parsed = parse_qr_payload(&normalize_qr_input(remote_qr))
            .map_err(|e| format!("invalid peer QR: {e}"))?;
        if parsed.public_key == self.identity.public_key_bytes() {
            return Err("that is your own QR code — paste the other peer's QR".into());
        }
        let canonical = peer_id_from_pubkey(&parsed.public_key);
        let pk_hex = hex::encode(parsed.public_key);
        self.user_paused.write().await.remove(&canonical);

        if self.is_peer_connected(&canonical).await && self.is_peer_trusted(&canonical).await {
            self.flush_pending_for_peer(&canonical).await;
            self.emit(EngineEvent::PeerConnected {
                peer_id: canonical.clone(),
                transport: TransportKind::Relay,
            })
            .await;
            let _ = self.broadcast_fresh_qr(&canonical).await;
            return Ok((canonical, String::new(), true));
        }

        let rx = self.register_handshake_waiter(&pk_hex).await;
        if !self.dials_in_progress.write().await.insert(pk_hex.clone()) {
            return self.await_handshake(rx).await;
        }
        let result = self.dial_and_handshake(&parsed, rx).await;
        self.dials_in_progress.write().await.remove(&pk_hex);
        result
    }

    async fn dial_and_handshake(
        &self,
        parsed: &ParsedQr,
        mut rx: oneshot::Receiver<HandshakeOutcome>,
    ) -> Result<(String, String, bool), String> {
        let conn_peer_id = self.ensure_connected(parsed).await?;
        match self.handshake_with_qr_bytes(&conn_peer_id, &parsed.public_key).await {
            Ok(v) => Ok(v),
            // The peer is running the handshake towards us; we finish as responder.
            Err(e) if e == SUPERSEDED => self.await_handshake(rx).await,
            Err(e) => match rx.try_recv() {
                // The peer's handshake completed while we were dialing (session already moved
                // to its canonical id) — that result is the answer.
                Ok(outcome) => outcome,
                Err(_) => Err(e),
            },
        }
    }

    async fn register_handshake_waiter(&self, pk_hex: &str) -> oneshot::Receiver<HandshakeOutcome> {
        let (tx, rx) = oneshot::channel();
        self.handshake_done
            .write()
            .await
            .entry(pk_hex.to_string())
            .or_default()
            .push(tx);
        rx
    }

    async fn notify_handshake_done(&self, remote_pk: &[u8; 32], outcome: HandshakeOutcome) {
        let waiters = self
            .handshake_done
            .write()
            .await
            .remove(&hex::encode(remote_pk))
            .unwrap_or_default();
        for tx in waiters {
            let _ = tx.send(outcome.clone());
        }
    }

    async fn await_handshake(&self, rx: oneshot::Receiver<HandshakeOutcome>) -> Result<(String, String, bool), String> {
        match tokio::time::timeout(HANDSHAKE_TIMEOUT + Duration::from_secs(15), rx).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => Err("handshake aborted".into()),
            Err(_) => Err("handshake timed out waiting for peer".into()),
        }
    }

    /// Run hybrid KEX over the wire; remote QR identity must match signed handshake.
    pub async fn handshake_with(&self, peer_id: &str, remote_qr: &str) -> Result<String, String> {
        let parsed = parse_qr_payload(&normalize_qr_input(remote_qr)).map_err(|e| e.to_string())?;
        let resolved = self.resolve_peer_id(peer_id).await?;
        let (_, sas, _) = self
            .handshake_with_qr_bytes(&resolved, &parsed.public_key)
            .await?;
        Ok(sas)
    }

    /// Reset the initiator state after a failed handshake (only if we are still initiator).
    async fn abort_initiator(&self, peer_id: &str) {
        self.handshake_wait.write().await.remove(peer_id);
        if let Some(session) = self.peers.write().await.get_mut(peer_id) {
            if session.role == Some(HandshakeRole::Initiator) {
                session.crypto = PeerCrypto::new_connected();
                session.pending_kex = None;
                session.role = None;
                session.step1 = None;
            }
        }
    }

    async fn handshake_with_qr_bytes(
        &self,
        peer_id: &str,
        expected_remote_pk: &[u8; 32],
    ) -> Result<(String, String, bool), String> {
        let (tx, rx) = oneshot::channel();
        let frame1 = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(peer_id)
                .ok_or_else(|| format!("peer not connected: {peer_id}"))?;
            if session.role == Some(HandshakeRole::Responder) {
                // The peer's handshake reached us first; finish that one instead.
                return Err(SUPERSEDED.into());
            }
            session.crypto = PeerCrypto::new_connected();
            session.answered_step1 = None;
            session.step2 = None;
            let (step1_body, kex) = session.crypto.begin_initiator();
            session
                .crypto
                .record_initiator_step1(&step1_body)
                .map_err(|e| format!("handshake step 1: {e}"))?;
            session.pending_kex = Some(kex);
            session.role = Some(HandshakeRole::Initiator);
            let frame = PeerCrypto::sign_handshake(&self.identity, 1, step1_body);
            session.step1 = Some(frame.clone());
            frame
        };
        self.handshake_wait
            .write()
            .await
            .insert(peer_id.to_string(), tx);

        if let Err(e) = self.send_raw_frame(peer_id, &WireFrame::Handshake(frame1)).await {
            self.abort_initiator(peer_id).await;
            return Err(e);
        }

        let resp = match tokio::time::timeout(HANDSHAKE_TIMEOUT, rx).await {
            Ok(Ok(HandshakeSignal::Step2(hs))) => hs,
            Ok(Ok(HandshakeSignal::Superseded)) => return Err(SUPERSEDED.into()),
            Ok(Err(_)) => {
                self.abort_initiator(peer_id).await;
                return Err("handshake aborted — connection closed".into());
            }
            Err(_) => {
                self.abort_initiator(peer_id).await;
                return Err("handshake timed out waiting for peer".into());
            }
        };

        let step = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(peer_id)
                .ok_or_else(|| format!("peer not found: {peer_id}"))?;
            if session.role != Some(HandshakeRole::Initiator) {
                return Err("handshake state lost".into());
            }
            let result = (|| {
                let mut kex = session
                    .pending_kex
                    .take()
                    .ok_or_else(|| "handshake state lost".to_string())?;
                let step3_body = session
                    .crypto
                    .initiator_process_step2(&mut kex, &resp, expected_remote_pk)?;
                let finish = PeerCrypto::sign_handshake(&self.identity, 3, step3_body.clone());
                let sas = session
                    .crypto
                    .initiator_finalize_step3(&self.identity, &step3_body)?;
                Ok::<_, String>((finish, sas))
            })();
            session.role = None;
            session.step1 = None;
            if result.is_err() {
                session.crypto = PeerCrypto::new_connected();
            }
            result
        };
        let (finish_frame, sas) = match step {
            Ok(v) => v,
            Err(e) => {
                self.notify_handshake_done(expected_remote_pk, Err(e.clone())).await;
                return Err(e);
            }
        };

        self.send_raw_frame(peer_id, &WireFrame::Handshake(finish_frame))
            .await?;

        let canonical = self.canonicalize_peer(peer_id).await?;
        self.user_paused.write().await.remove(&canonical);
        let auto_trusted = self.maybe_auto_trust(&canonical, expected_remote_pk).await;

        self.emit(EngineEvent::SasReady {
            peer_id: canonical.clone(),
            sas: sas.clone(),
            auto_trusted,
        })
        .await;
        let outcome = (canonical, sas, auto_trusted);
        self.notify_handshake_done(expected_remote_pk, Ok(outcome.clone())).await;
        Ok(outcome)
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
                .insert(hex::encode(remote_pk));
        }
        self.user_paused.write().await.remove(&resolved);
        let _ = self.send_ratchet_bootstrap(&resolved).await;
        self.release_held_inbound(&resolved).await;
        let _ = self.broadcast_profile(&resolved).await;
        let _ = self.broadcast_fresh_qr(&resolved).await;
        self.flush_pending_for_peer(&resolved).await;
        Ok(())
    }

    pub async fn is_peer_trusted(&self, peer_id: &str) -> bool {
        match self.resolve_peer_id(peer_id).await {
            Ok(resolved) => self
                .peers
                .read()
                .await
                .get(&resolved)
                .is_some_and(|s| s.crypto.is_trusted()),
            Err(_) => false,
        }
    }

    /// Errors after which a message is queued and delivered later: the link is down, or we
    /// are the responder and the initiator's ratchet-open has not arrived yet (it is flushed
    /// from the ratchet_open handler moments later).
    fn is_link_error(e: &str) -> bool {
        e.contains("not connected")
            || e.contains("reconnect")
            || e.contains("not found")
            || e.contains("ratchet send chain not ready")
    }

    pub async fn send_message(&self, peer_id: &str, content: &str) -> Result<(), String> {
        let resolved = self
            .resolve_peer_id(peer_id)
            .await
            .unwrap_or_else(|_| peer_id.to_string());
        match self.try_send_message_now(&resolved, content).await {
            Ok(()) => Ok(()),
            Err(e) if Self::is_link_error(&e) && self.can_queue_for_peer(&resolved).await => {
                let size = self
                    .enqueue_pending(&resolved, PendingOutbound::Text(content.to_string()))
                    .await?;
                self.emit(EngineEvent::MessageQueued {
                    peer_id: resolved.clone(),
                    queue_size: size,
                })
                .await;
                if !self.link_up(&resolved).await {
                    self.schedule_auto_reconnect(resolved);
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    async fn try_send_message_now(&self, peer_id: &str, content: &str) -> Result<(), String> {
        self.send_wire_message(peer_id, ChatMessage::text(&self.public_key_hex(), peer_id, content))
            .await
    }

    pub async fn send_file(&self, peer_id: &str, file_path: &str) -> Result<(String, String, f64), String> {
        let resolved = self
            .resolve_peer_id(peer_id)
            .await
            .unwrap_or_else(|_| peer_id.to_string());
        match self.try_send_file_now(&resolved, file_path).await {
            Ok(v) => Ok(v),
            Err(e) if Self::is_link_error(&e) && self.can_queue_for_peer(&resolved).await => {
                let size = self
                    .enqueue_pending(&resolved, PendingOutbound::File(PathBuf::from(file_path)))
                    .await?;
                self.emit(EngineEvent::MessageQueued {
                    peer_id: resolved.clone(),
                    queue_size: size,
                })
                .await;
                if !self.link_up(&resolved).await {
                    self.schedule_auto_reconnect(resolved);
                }
                Ok((String::new(), format!("queued:{file_path}"), 0.0))
            }
            Err(e) => Err(e),
        }
    }

    async fn try_send_file_now(&self, peer_id: &str, file_path: &str) -> Result<(String, String, f64), String> {
        let resolved = self.resolve_peer_id(peer_id).await?;
        if !self.link_up(&resolved).await {
            return Err("peer not connected — reconnect from Saved Peers".into());
        }
        if !self.is_peer_trusted(&resolved).await {
            return Err("peer not trusted — confirm SAS first".into());
        }
        let path = PathBuf::from(file_path);
        if !Path::new(file_path).is_file() {
            return Err(format!("file not found: {file_path}"));
        }
        let serial = self
            .peers
            .read()
            .await
            .get(&resolved)
            .is_some_and(|s| s.transport == TransportKind::Serial);
        let chunk_size = if serial { DEFAULT_CHUNK_SIZE } else { IROH_CHUNK_SIZE };
        // Hashing a large file is blocking work.
        let sender = tokio::task::spawn_blocking(move || ChunkedSender::from_file(&path, chunk_size))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let transfer_id = sender.manifest.id.to_string();
        let filename = sender.manifest.filename.clone();
        let progress = sender.progress();
        let manifest = sender.manifest.clone();

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
                "chunk_size": manifest.chunk_size,
                "sha256": manifest.sha256,
            })),
        };
        self.transfers.write().await.insert(
            transfer_id.clone(),
            ActiveTransfer {
                sender,
                peer_id: resolved.clone(),
            },
        );
        if let Err(e) = self.send_wire_message(&resolved, notice).await {
            self.transfers.write().await.remove(&transfer_id);
            return Err(e);
        }

        self.emit(EngineEvent::TransferProgress {
            id: transfer_id.clone(),
            filename: filename.clone(),
            progress,
            peer_id: resolved.clone(),
            total_bytes: manifest.total_size,
        })
        .await;

        let engine = self.clone();
        let tid = transfer_id.clone();
        tokio::spawn(async move {
            engine.run_transfer_send(resolved, tid, serial).await;
        });
        Ok((transfer_id, filename, progress))
    }

    /// Cancel an outgoing or incoming transfer and tell the peer.
    pub async fn cancel_transfer(&self, transfer_id: &str) -> Result<(), String> {
        self.cancelled_transfers
            .write()
            .await
            .insert(transfer_id.to_string());
        let outgoing = self.transfers.write().await.remove(transfer_id);
        let incoming = self.incoming_transfers.write().await.remove(transfer_id);
        let (peer_id, filename) = match (outgoing, incoming) {
            (Some(t), _) => (t.peer_id, t.sender.manifest.filename),
            (None, Some(t)) => {
                let name = t.receiver.manifest.filename.clone();
                let peer = t.peer_id.clone();
                t.receiver.discard();
                (peer, name)
            }
            (None, None) => return Ok(()),
        };
        let msg = self.file_message(
            &peer_id,
            serde_json::json!({ "transfer_id": transfer_id, "action": "cancel" }),
        );
        let _ = self.send_wire_message(&peer_id, msg).await;
        self.emit(EngineEvent::TransferCancelled {
            id: transfer_id.to_string(),
            filename,
            peer_id,
        })
        .await;
        Ok(())
    }

    async fn run_transfer_send(&self, peer_id: String, transfer_id: String, serial: bool) {
        let (window, resend_after) = if serial {
            (4, Duration::from_secs(30))
        } else {
            (16, Duration::from_secs(10))
        };
        loop {
            if self.cancelled_transfers.read().await.contains(&transfer_id) {
                self.transfers.write().await.remove(&transfer_id);
                return;
            }
            let step = {
                let mut transfers = self.transfers.write().await;
                let Some(active) = transfers.get_mut(&transfer_id) else {
                    return;
                };
                let filename = active.sender.manifest.filename.clone();
                if active.sender.is_complete() {
                    transfers.remove(&transfer_id);
                    TransferStep::Done(filename)
                } else if active.sender.idle_for() > TRANSFER_STALL {
                    transfers.remove(&transfer_id);
                    TransferStep::Fail(filename, "peer stopped acknowledging".into())
                } else {
                    match active.sender.next_chunks(window, resend_after) {
                        Ok(chunks) => TransferStep::Send(
                            chunks,
                            filename,
                            active.sender.progress(),
                            active.sender.manifest.total_size,
                        ),
                        Err(e) => {
                            transfers.remove(&transfer_id);
                            TransferStep::Fail(filename, format!("read error: {e}"))
                        }
                    }
                }
            };

            let (chunks, filename) = match step {
                TransferStep::Done(filename) => {
                    self.emit(EngineEvent::TransferComplete {
                        id: transfer_id,
                        filename,
                        peer_id,
                        path: String::new(),
                    })
                    .await;
                    return;
                }
                TransferStep::Fail(filename, reason) => {
                    let msg = self.file_message(
                        &peer_id,
                        serde_json::json!({ "transfer_id": transfer_id, "action": "cancel" }),
                    );
                    let _ = self.send_wire_message(&peer_id, msg).await;
                    self.emit(EngineEvent::TransferFailed {
                        id: transfer_id,
                        filename,
                        peer_id,
                        reason,
                    })
                    .await;
                    return;
                }
                TransferStep::Send(chunks, filename, progress, total_bytes) => {
                    self.emit_lossy(EngineEvent::TransferProgress {
                        id: transfer_id.clone(),
                        filename: filename.clone(),
                        progress,
                        peer_id: peer_id.clone(),
                        total_bytes,
                    });
                    (chunks, filename)
                }
            };

            if chunks.is_empty() {
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            }
            for (chunk_id, data) in chunks {
                if self.cancelled_transfers.read().await.contains(&transfer_id) {
                    return;
                }
                let msg = self.file_message(
                    &peer_id,
                    serde_json::json!({
                        "transfer_id": transfer_id,
                        "action": "chunk",
                        "chunk_id": chunk_id,
                        "data_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data),
                    }),
                );
                if let Err(e) = self.send_wire_message(&peer_id, msg).await {
                    if !self.link_up(&peer_id).await {
                        if self.transfers.write().await.remove(&transfer_id).is_some() {
                            self.emit(EngineEvent::TransferFailed {
                                id: transfer_id,
                                filename,
                                peer_id,
                                reason: "connection lost".into(),
                            })
                            .await;
                        }
                        return;
                    }
                    // Chunk stays in flight and is resent after `resend_after`.
                    warn!(transfer_id = %transfer_id, error = %e, "chunk send failed — will retry");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
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
            "answer" | "end" => MessageType::CallAnswer,
            "ice" => MessageType::CallIce,
            _ => return Err(format!("unknown call signal: {signal}")),
        };
        if signal == "offer" {
            let mut peer_calls = self.active_peer_calls.write().await;
            if let Some(existing) = peer_calls.get(&resolved) {
                if existing != call_id {
                    return Err("already in a call with this peer".into());
                }
            } else {
                peer_calls.insert(resolved.clone(), call_id.to_string());
                self.calls.write().await.insert(
                    call_id.to_string(),
                    ActiveCall {
                        session: CallSession::new(&resolved, is_video),
                    },
                );
            }
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
        let sent = self.send_wire_message(&resolved, msg).await;
        if signal == "end" || (signal == "offer" && sent.is_err()) {
            self.clear_call_for_peer(&resolved, call_id).await;
        }
        sent
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
            .await
    }

    /// Encrypt and send an application message to a trusted, connected peer.
    async fn send_wire_message(&self, peer_id: &str, msg: ChatMessage) -> Result<(), String> {
        let resolved = self.resolve_peer_id(peer_id).await?;
        if !self.link_up(&resolved).await {
            return Err("peer not connected — reconnect from Saved Peers".into());
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
        self.send_raw_frame(&resolved, &WireFrame::Encrypted(payload))
            .await
    }

    async fn send_raw_frame(&self, peer_id: &str, frame: &WireFrame) -> Result<(), String> {
        let wire = frame.serialize().map_err(|e| e.to_string())?;
        let found = self.peers.read().await.get(peer_id).map(|s| s.transport);
        let (key, transport) = match found {
            Some(t) => (peer_id.to_string(), t),
            None => {
                let resolved = self.resolve_peer_id(peer_id).await?;
                let transport = self
                    .peers
                    .read()
                    .await
                    .get(&resolved)
                    .map(|s| s.transport)
                    .ok_or_else(|| format!("peer not found: {resolved}"))?;
                (resolved, transport)
            }
        };
        match transport {
            TransportKind::Serial => {
                let serial = self
                    .serial
                    .read()
                    .await
                    .clone()
                    .ok_or_else(|| "serial transport not connected".to_string())?;
                serial.send(bytes::Bytes::from(wire)).await
            }
            TransportKind::Lan | TransportKind::Wan | TransportKind::Relay => self
                .iroh
                .read()
                .await
                .send(&key, &wire)
                .await
                .map_err(|e| e.to_string()),
        }
    }

    pub fn available_serial_ports() -> Vec<crate::serial::SerialPortEntry> {
        list_ports()
    }

    pub async fn shutdown(&self) {
        {
            let mut running = self.running.write().await;
            if !*running {
                return;
            }
            *running = false;
        }
        info!("P2P engine shutting down gracefully");

        let serial = self.serial.write().await.take();
        if let Some(t) = serial {
            t.shutdown().await;
        }
        self.iroh.write().await.shutdown().await;
        self.peers.write().await.clear();
        self.transfers.write().await.clear();
        let incoming: Vec<IncomingTransfer> = self
            .incoming_transfers
            .write()
            .await
            .drain()
            .map(|(_, t)| t)
            .collect();
        for t in incoming {
            t.receiver.discard();
        }
        self.calls.write().await.clear();
        self.active_peer_calls.write().await.clear();
        self.handshake_wait.write().await.clear();
        self.handshake_done.write().await.clear();
        self.held_inbound.write().await.clear();

        self.emit(EngineEvent::Stopped).await;
        info!("P2P engine stopped");
    }

    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Session ids whose transport link is currently up.
    pub async fn connected_peers(&self) -> Vec<String> {
        let ids: Vec<String> = self.peers.read().await.keys().cloned().collect();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if self.link_up(&id).await {
                out.push(id);
            }
        }
        out
    }

    // ── Inbound ─────────────────────────────────────────────────────────

    async fn handle_inbound_bytes(&self, peer_id: &str, data: &[u8]) {
        let resolved = self
            .resolve_peer_id(peer_id)
            .await
            .unwrap_or_else(|_| peer_id.to_string());
        match WireFrame::deserialize(data) {
            Ok(WireFrame::Handshake(hs)) => self.handle_handshake_frame(&resolved, hs).await,
            Ok(WireFrame::Encrypted(enc)) => self.handle_encrypted_frame(&resolved, enc).await,
            // Unauthenticated plaintext is never accepted (no legacy JSON chat fallback).
            Err(e) => debug!(peer = %resolved, error = %e, "dropping undecodable frame"),
        }
    }

    async fn handle_handshake_frame(&self, peer_id: &str, hs: SignedHandshake) {
        match hs.step {
            1 => self.handle_handshake_init(peer_id, hs).await,
            2 => {
                let waiter = self.handshake_wait.write().await.remove(peer_id);
                match waiter {
                    Some(tx) => {
                        let _ = tx.send(HandshakeSignal::Step2(hs));
                    }
                    None => debug!(peer = %peer_id, "unexpected handshake step 2"),
                }
            }
            3 => self.handle_handshake_finish(peer_id, hs).await,
            _ => warn!(step = hs.step, "unknown handshake step"),
        }
    }

    async fn handle_handshake_init(&self, peer_id: &str, init: SignedHandshake) {
        enum Decision {
            Ignore,
            Resend(SignedHandshake),
            Respond(SignedHandshake, bool),
            Fail(String),
        }
        if PeerCrypto::verify_handshake_frame(&init).is_err() {
            warn!(peer = %peer_id, "handshake step 1 with bad signature — ignored");
            return;
        }
        let decision = {
            let mut peers = self.peers.write().await;
            let session = peers
                .entry(peer_id.to_string())
                .or_insert_with(|| PeerSession::new(TransportKind::Lan));
            let pinned = session.crypto.remote_identity;
            let duplicate = session.answered_step1.as_deref() == Some(init.signature.as_slice());
            if duplicate {
                match (&session.role, &session.step2) {
                    (Some(HandshakeRole::Responder), Some(step2)) => Decision::Resend(step2.clone()),
                    _ => Decision::Ignore, // already finished with this exact handshake
                }
            } else if peer_id.starts_with("peer:") && pinned != [0u8; 32] && pinned != init.identity {
                Decision::Fail("handshake identity does not match this peer — ignored".into())
            } else if session.role == Some(HandshakeRole::Initiator)
                && self.identity.public_key_bytes() < init.identity
            {
                // Both sides started at once: the smaller identity stays initiator and the
                // peer yields when it receives our step 1.
                Decision::Ignore
            } else {
                let yielded = session.role == Some(HandshakeRole::Initiator);
                session.crypto = PeerCrypto::new_connected();
                session.pending_kex = None;
                session.step1 = None;
                match session.crypto.responder_process_step1(&self.identity, &init) {
                    Ok((resp, kex)) => {
                        session.pending_kex = Some(kex);
                        session.role = Some(HandshakeRole::Responder);
                        session.answered_step1 = Some(init.signature.clone());
                        session.step2 = Some(resp.clone());
                        Decision::Respond(resp, yielded)
                    }
                    Err(e) => {
                        session.role = None;
                        Decision::Fail(e)
                    }
                }
            }
        };
        match decision {
            Decision::Ignore => debug!(peer = %peer_id, "handshake step 1 ignored (glare or duplicate)"),
            Decision::Resend(step2) => {
                let _ = self.send_raw_frame(peer_id, &WireFrame::Handshake(step2)).await;
            }
            Decision::Respond(resp, yielded) => {
                if yielded {
                    let waiter = self.handshake_wait.write().await.remove(peer_id);
                    if let Some(tx) = waiter {
                        let _ = tx.send(HandshakeSignal::Superseded);
                    }
                }
                if let Err(e) = self.send_raw_frame(peer_id, &WireFrame::Handshake(resp)).await {
                    warn!(error = %e, "failed to send handshake response");
                }
            }
            Decision::Fail(e) => self.emit(EngineEvent::Error(e)).await,
        }
    }

    async fn handle_handshake_finish(&self, peer_id: &str, finish: SignedHandshake) {
        let sas_result = {
            let mut peers = self.peers.write().await;
            match peers.get_mut(peer_id) {
                None => Err(format!("peer not found: {peer_id}")),
                Some(session) if session.role != Some(HandshakeRole::Responder) => {
                    Err("unexpected handshake finish".to_string())
                }
                Some(session) => {
                    session.pending_kex = None;
                    session.role = None;
                    session.step2 = None;
                    let r = session.crypto.responder_process_step3(&self.identity, &finish);
                    if r.is_err() {
                        session.crypto = PeerCrypto::new_connected();
                    }
                    r.map(|sas| (sas, session.crypto.remote_identity))
                }
            }
        };
        let (sas, remote_pk) = match sas_result {
            Ok(v) => v,
            Err(e) => {
                warn!(peer = %peer_id, error = %e, "handshake finish failed");
                self.emit(EngineEvent::Error(e)).await;
                return;
            }
        };
        let canonical = self
            .canonicalize_peer(peer_id)
            .await
            .unwrap_or_else(|_| peer_id.to_string());
        let auto_trusted = self.trusted_pubkeys.read().await.contains(&hex::encode(remote_pk))
            && self
                .peers
                .write()
                .await
                .get_mut(&canonical)
                .is_some_and(|s| s.crypto.confirm_trusted().is_ok());
        // Responder cannot send until the initiator's ratchet_open arrives; profile, QR and
        // queued items are sent from that handler.
        self.user_paused.write().await.remove(&canonical);
        self.emit(EngineEvent::SasReady {
            peer_id: canonical.clone(),
            sas: sas.clone(),
            auto_trusted,
        })
        .await;
        self.notify_handshake_done(&remote_pk, Ok((canonical, sas, auto_trusted)))
            .await;
    }

    async fn handle_encrypted_frame(&self, peer_id: &str, enc: EncryptedPayload) {
        let decrypted = {
            let mut peers = self.peers.write().await;
            match peers.get_mut(peer_id) {
                None => Err(format!("peer not found: {peer_id}")),
                Some(session) => session.crypto.decrypt(&enc.ciphertext).map(|p| {
                    (
                        p,
                        session.crypto.is_trusted(),
                        session.crypto.can_send_encrypted(),
                        session.crypto.remote_identity,
                    )
                }),
            }
        };
        let (plaintext, trusted, can_send, remote_pk) = match decrypted {
            Ok(v) => v,
            Err(e) => {
                warn!(peer = %peer_id, error = %e, "decrypt failed — frame dropped");
                return;
            }
        };
        let Ok(mut msg) = ChatMessage::from_json(&plaintext) else {
            warn!(peer = %peer_id, "undecodable message — dropped");
            return;
        };
        // The authenticated session identity is the sender; never trust the claimed field.
        msg.sender_id = hex::encode(remote_pk);

        let action = msg
            .metadata
            .as_ref()
            .and_then(|m| m.get("action"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if action.as_deref() == Some("ratchet_open") {
            if trusted && can_send {
                let _ = self.broadcast_profile(peer_id).await;
                let _ = self.broadcast_fresh_qr(peer_id).await;
                self.flush_pending_for_peer(peer_id).await;
            }
            return;
        }
        if !trusted {
            let mut held = self.held_inbound.write().await;
            let q = held.entry(peer_id.to_string()).or_default();
            if q.len() < MAX_HELD_INBOUND {
                q.push(msg);
            } else {
                warn!(peer = %peer_id, "holding queue full for unverified peer — message dropped");
            }
            return;
        }
        self.handle_app_message(peer_id, msg).await;
    }

    /// Deliver messages that arrived before the user confirmed the SAS.
    async fn release_held_inbound(&self, peer_id: &str) {
        let held = self
            .held_inbound
            .write()
            .await
            .remove(peer_id)
            .unwrap_or_default();
        for msg in held {
            self.handle_app_message(peer_id, msg).await;
        }
    }

    async fn handle_app_message(&self, peer_id: &str, msg: ChatMessage) {
        let meta = msg.metadata.clone().unwrap_or(serde_json::Value::Null);
        let action = meta.get("action").and_then(|v| v.as_str()).unwrap_or("");
        match msg.msg_type {
            MessageType::File => match action {
                "offer" => self.on_file_offer(peer_id, &msg, &meta).await,
                "chunk" => self.on_file_chunk(peer_id, &meta).await,
                "ack" => {
                    let tid = meta.get("transfer_id").and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(ack) = meta
                        .get("ack")
                        .and_then(|v| serde_json::from_value::<ChunkAck>(v.clone()).ok())
                    {
                        let mut transfers = self.transfers.write().await;
                        if let Some(active) = transfers.get_mut(tid).filter(|t| t.peer_id == peer_id) {
                            active.sender.on_ack(&ack);
                        }
                    }
                }
                "cancel" => {
                    let tid = meta.get("transfer_id").and_then(|v| v.as_str()).unwrap_or("");
                    self.on_remote_cancel(peer_id, tid).await;
                }
                _ => debug!(action, "unknown file action"),
            },
            MessageType::CallOffer | MessageType::CallAnswer | MessageType::CallIce => {
                self.on_call_signal(peer_id, &msg, &meta).await;
            }
            MessageType::System => match action {
                "profile" => {
                    let name: String = msg.content.trim().chars().take(64).collect();
                    if !name.is_empty() {
                        self.remote_display_names
                            .write()
                            .await
                            .insert(peer_id.to_string(), name.clone());
                        self.emit(EngineEvent::PeerProfile {
                            peer_id: peer_id.to_string(),
                            display_name: name,
                        })
                        .await;
                    }
                }
                "qr_refresh" => {
                    let qr = msg.content.trim().to_string();
                    // Reject identity swaps via ticket refresh — only the same Ed25519 key.
                    match parse_qr_payload(&qr) {
                        Ok(parsed) if peer_id_from_pubkey(&parsed.public_key) == peer_id => {
                            self.saved_peer_qr
                                .write()
                                .await
                                .insert(peer_id.to_string(), qr.clone());
                            self.emit(EngineEvent::PeerQrRefresh {
                                peer_id: peer_id.to_string(),
                                qr,
                            })
                            .await;
                        }
                        Ok(_) => warn!(peer = %peer_id, "qr_refresh identity mismatch — ignored"),
                        Err(e) => warn!(error = %e, "qr_refresh parse failed — ignored"),
                    }
                }
                _ => debug!(action, "unknown system action"),
            },
            _ => self.emit(EngineEvent::MessageReceived(msg)).await,
        }
    }

    async fn on_file_offer(&self, peer_id: &str, msg: &ChatMessage, meta: &serde_json::Value) {
        let raw_id = meta.get("transfer_id").and_then(|v| v.as_str()).unwrap_or("");
        let Ok(id) = uuid::Uuid::parse_str(raw_id) else {
            warn!("file offer with invalid transfer id");
            return;
        };
        let transfer_id = id.to_string();
        if self.incoming_transfers.read().await.contains_key(&transfer_id)
            || self.completed_incoming.read().await.contains(&transfer_id)
        {
            return; // duplicate offer
        }
        let filename = sanitize_filename(
            meta.get("filename").and_then(|v| v.as_str()).unwrap_or(&msg.content),
        );
        let manifest = TransferManifest {
            id,
            filename: filename.clone(),
            total_size: meta.get("total_size").and_then(|v| v.as_u64()).unwrap_or(0),
            chunk_size: meta
                .get("chunk_size")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(DEFAULT_CHUNK_SIZE),
            total_chunks: meta
                .get("total_chunks")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .min(u32::MAX as u64) as u32,
            sha256: meta.get("sha256").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        };
        let total_bytes = manifest.total_size;
        let dir = self.receive_dir.read().await.clone();
        let storage = Self::transfer_storage_name(&transfer_id, &filename);
        let receiver = match ChunkedReceiver::create(manifest, &dir, &storage) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "rejecting file offer");
                let cancel = self.file_message(
                    peer_id,
                    serde_json::json!({ "transfer_id": transfer_id, "action": "cancel" }),
                );
                let _ = self.send_wire_message(peer_id, cancel).await;
                self.emit(EngineEvent::TransferFailed {
                    id: transfer_id,
                    filename,
                    peer_id: peer_id.to_string(),
                    reason: e.to_string(),
                })
                .await;
                return;
            }
        };
        if receiver.is_complete() {
            // Empty file: nothing to wait for.
            self.finish_incoming(peer_id, transfer_id, filename, receiver).await;
            return;
        }
        self.incoming_transfers.write().await.insert(
            transfer_id.clone(),
            IncomingTransfer {
                receiver,
                peer_id: peer_id.to_string(),
            },
        );
        self.emit(EngineEvent::TransferProgress {
            id: transfer_id,
            filename,
            progress: 0.0,
            peer_id: peer_id.to_string(),
            total_bytes,
        })
        .await;
    }

    async fn on_file_chunk(&self, peer_id: &str, meta: &serde_json::Value) {
        let tid = meta.get("transfer_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let Some(chunk_id) = meta
            .get("chunk_id")
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok())
        else {
            return;
        };
        let data_b64 = meta.get("data_b64").and_then(|v| v.as_str()).unwrap_or("");
        let Ok(data) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data_b64) else {
            return;
        };
        let already_done = self.completed_incoming.read().await.contains(&tid);

        let outcome = {
            let mut incoming = self.incoming_transfers.write().await;
            match incoming.get_mut(&tid) {
                None => match uuid::Uuid::parse_str(&tid) {
                    // Our ACK was lost after completion; re-acknowledge so the sender finishes.
                    Ok(id) if already_done => ChunkOutcome::Ack(
                        ChunkAck {
                            transfer_id: id,
                            received_chunks: vec![chunk_id],
                            missing_chunks: vec![],
                        },
                        None,
                    ),
                    _ => ChunkOutcome::Ignore,
                },
                Some(t) if t.peer_id != peer_id => ChunkOutcome::Ignore,
                Some(t) => match t.receiver.receive_chunk(chunk_id, &data) {
                    Ok(false) => ChunkOutcome::Ignore,
                    Err(e) => {
                        let filename = t.receiver.manifest.filename.clone();
                        if let Some(t) = incoming.remove(&tid) {
                            t.receiver.discard();
                        }
                        ChunkOutcome::Fail(filename, format!("write failed: {e}"))
                    }
                    Ok(true) => {
                        let ack = t.receiver.ack_for(chunk_id);
                        if t.receiver.is_complete() {
                            match incoming.remove(&tid) {
                                Some(t) => ChunkOutcome::Complete(ack, Box::new(t.receiver)),
                                None => ChunkOutcome::Ignore,
                            }
                        } else {
                            let info = (
                                t.receiver.manifest.filename.clone(),
                                t.receiver.progress(),
                                t.receiver.manifest.total_size,
                            );
                            ChunkOutcome::Ack(ack, Some(info))
                        }
                    }
                },
            }
        };

        let ack_msg = |ack: &ChunkAck| {
            self.file_message(
                peer_id,
                serde_json::json!({ "transfer_id": tid, "action": "ack", "ack": ack }),
            )
        };
        match outcome {
            ChunkOutcome::Ignore => {}
            ChunkOutcome::Ack(ack, info) => {
                let _ = self.send_wire_message(peer_id, ack_msg(&ack)).await;
                if let Some((filename, progress, total_bytes)) = info {
                    self.emit_lossy(EngineEvent::TransferProgress {
                        id: tid,
                        filename,
                        progress,
                        peer_id: peer_id.to_string(),
                        total_bytes,
                    });
                }
            }
            ChunkOutcome::Complete(ack, receiver) => {
                let _ = self.send_wire_message(peer_id, ack_msg(&ack)).await;
                let filename = receiver.manifest.filename.clone();
                self.finish_incoming(peer_id, tid, filename, *receiver).await;
            }
            ChunkOutcome::Fail(filename, reason) => {
                let cancel = self.file_message(
                    peer_id,
                    serde_json::json!({ "transfer_id": tid, "action": "cancel" }),
                );
                let _ = self.send_wire_message(peer_id, cancel).await;
                self.emit(EngineEvent::TransferFailed {
                    id: tid,
                    filename,
                    peer_id: peer_id.to_string(),
                    reason,
                })
                .await;
            }
        }
    }

    /// Verify and move a completed incoming file into place (hashing runs off the runtime).
    async fn finish_incoming(&self, peer_id: &str, tid: String, filename: String, receiver: ChunkedReceiver) {
        self.completed_incoming.write().await.insert(tid.clone());
        let result = tokio::task::spawn_blocking(move || receiver.finalize())
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()));
        match result {
            Ok(path) => {
                self.emit(EngineEvent::TransferComplete {
                    id: tid,
                    filename,
                    peer_id: peer_id.to_string(),
                    path: path.to_string_lossy().to_string(),
                })
                .await
            }
            Err(reason) => {
                self.emit(EngineEvent::TransferFailed {
                    id: tid,
                    filename,
                    peer_id: peer_id.to_string(),
                    reason,
                })
                .await
            }
        }
    }

    async fn on_remote_cancel(&self, peer_id: &str, tid: &str) {
        let incoming = {
            let mut map = self.incoming_transfers.write().await;
            if map.get(tid).is_some_and(|t| t.peer_id == peer_id) {
                map.remove(tid)
            } else {
                None
            }
        };
        let outgoing = {
            let mut map = self.transfers.write().await;
            if map.get(tid).is_some_and(|t| t.peer_id == peer_id) {
                map.remove(tid)
            } else {
                None
            }
        };
        let filename = match (incoming, outgoing) {
            (Some(t), _) => {
                let name = t.receiver.manifest.filename.clone();
                t.receiver.discard();
                name
            }
            (None, Some(t)) => {
                self.cancelled_transfers.write().await.insert(tid.to_string());
                t.sender.manifest.filename
            }
            (None, None) => return,
        };
        self.emit(EngineEvent::TransferCancelled {
            id: tid.to_string(),
            filename,
            peer_id: peer_id.to_string(),
        })
        .await;
    }

    async fn on_call_signal(&self, peer_id: &str, msg: &ChatMessage, meta: &serde_json::Value) {
        let call_id = meta.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if call_id.is_empty() {
            return;
        }
        let signal = meta
            .get("signal")
            .and_then(|v| v.as_str())
            .unwrap_or(match msg.msg_type {
                MessageType::CallOffer => "offer",
                MessageType::CallAnswer => "answer",
                _ => "ice",
            })
            .to_string();
        let is_video = meta.get("is_video").and_then(|v| v.as_bool()).unwrap_or(false);
        match signal.as_str() {
            "end" => self.clear_call_for_peer(peer_id, &call_id).await,
            "offer" => {
                let busy = self
                    .active_peer_calls
                    .read()
                    .await
                    .get(peer_id)
                    .is_some_and(|existing| existing != &call_id);
                if busy {
                    let end = ChatMessage {
                        id: uuid::Uuid::new_v4(),
                        sender_id: self.public_key_hex(),
                        recipient_id: peer_id.to_string(),
                        msg_type: MessageType::CallAnswer,
                        content: String::new(),
                        timestamp: chrono::Utc::now(),
                        metadata: Some(serde_json::json!({
                            "call_id": call_id,
                            "signal": "end",
                            "is_video": is_video,
                        })),
                    };
                    let _ = self.send_wire_message(peer_id, end).await;
                    return;
                }
                self.active_peer_calls
                    .write()
                    .await
                    .insert(peer_id.to_string(), call_id.clone());
                self.emit(EngineEvent::CallSignaling {
                    call_id,
                    peer_id: peer_id.to_string(),
                    signal,
                    payload: msg.content.clone(),
                    is_video,
                })
                .await;
            }
            "answer" | "ice" => {
                self.emit(EngineEvent::CallSignaling {
                    call_id,
                    peer_id: peer_id.to_string(),
                    signal,
                    payload: msg.content.clone(),
                    is_video,
                })
                .await
            }
            other => debug!(signal = other, "unknown call signal"),
        }
    }
}
