//! Resumable chunked file transfer with a sliding window and selective ACKs.
//!
//! The sender streams chunks from disk and keeps at most `window` chunks in flight,
//! resending a chunk only when its ACK has not arrived within `resend_after`. The receiver
//! writes each chunk at its offset in a `.part` file and verifies SHA-256 before the file
//! is moved into place, so neither side holds the whole file in memory.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tracing::info;
use uuid::Uuid;

/// Legacy chunk size (v0.3.x and serial links). Receivers assume it when an offer omits
/// `chunk_size`.
pub const DEFAULT_CHUNK_SIZE: usize = 4000;
/// Chunk size for iroh (QUIC) links.
pub const IROH_CHUNK_SIZE: usize = 64 * 1024;
/// Largest chunk size a receiver accepts from an offer.
pub const MAX_CHUNK_SIZE: usize = 1024 * 1024;
/// Largest single file a receiver accepts (64 GiB).
pub const MAX_TRANSFER_BYTES: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum TransferError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("transfer not found: {0}")]
    NotFound(Uuid),
    #[error("checksum mismatch — file corrupted in transit")]
    ChecksumMismatch,
    #[error("invalid transfer offer: {0}")]
    InvalidOffer(String),
    #[error("transfer incomplete")]
    Incomplete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferManifest {
    pub id: Uuid,
    pub filename: String,
    pub total_size: u64,
    pub chunk_size: usize,
    pub total_chunks: u32,
    pub sha256: String,
}

impl TransferManifest {
    /// Check that a remote-supplied manifest is self-consistent and within limits.
    pub fn validate(&self) -> Result<(), TransferError> {
        if self.chunk_size == 0 || self.chunk_size > MAX_CHUNK_SIZE {
            return Err(TransferError::InvalidOffer(format!("chunk size {}", self.chunk_size)));
        }
        if self.total_size > MAX_TRANSFER_BYTES {
            return Err(TransferError::InvalidOffer(format!("file too large ({} bytes)", self.total_size)));
        }
        let expected = self.total_size.div_ceil(self.chunk_size as u64);
        if expected != self.total_chunks as u64 {
            return Err(TransferError::InvalidOffer(format!(
                "{} chunks for {} bytes at {} per chunk",
                self.total_chunks, self.total_size, self.chunk_size
            )));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(TransferError::InvalidOffer("bad sha256".into()));
        }
        Ok(())
    }

    fn chunk_len(&self, id: u32) -> usize {
        let start = id as u64 * self.chunk_size as u64;
        (self.total_size - start).min(self.chunk_size as u64) as usize
    }
}

/// ACK for received chunks. v0.4+ receivers list only the chunk(s) just received; older
/// receivers sent the full set, which `on_ack` also handles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkAck {
    pub transfer_id: Uuid,
    pub received_chunks: Vec<u32>,
    #[serde(default)]
    pub missing_chunks: Vec<u32>,
}

/// Turn a peer-supplied filename into a single safe path component.
///
/// Strips directories (both separators), control characters and characters Windows
/// rejects, and never returns an empty name, `.` or `..`.
pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    let mut out = if trimmed.is_empty() { "file".to_string() } else { trimmed.to_string() };
    if out.len() > 200 {
        let mut cut = 200;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out
}

enum Source {
    File(File),
    Memory(Vec<u8>),
}

/// Sender side of a chunked transfer.
pub struct ChunkedSender {
    pub manifest: TransferManifest,
    source: Source,
    next_unsent: u32,
    inflight: HashMap<u32, Instant>,
    acked: HashSet<u32>,
    last_progress: Instant,
}

impl ChunkedSender {
    /// Open `path` for streaming; hashes the file once up front.
    pub fn from_file(path: &Path, chunk_size: usize) -> Result<Self, TransferError> {
        let mut file = File::open(path)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(TransferError::InvalidOffer("not a regular file".into()));
        }
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1024 * 1024];
        let mut total = 0u64;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            hasher.update(&buf[..n]);
        }
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        let manifest = Self::make_manifest(&filename, total, chunk_size, hex::encode(hasher.finalize()));
        info!(id = %manifest.id, file = %manifest.filename, chunks = manifest.total_chunks, "transfer created");
        Ok(Self::with_source(manifest, Source::File(file)))
    }

    pub fn from_bytes(filename: &str, data: Vec<u8>, chunk_size: usize) -> Self {
        let sha = hex::encode(Sha256::digest(&data));
        let manifest = Self::make_manifest(filename, data.len() as u64, chunk_size, sha);
        Self::with_source(manifest, Source::Memory(data))
    }

    fn make_manifest(filename: &str, total: u64, chunk_size: usize, sha256: String) -> TransferManifest {
        let chunk_size = chunk_size.clamp(1, MAX_CHUNK_SIZE);
        TransferManifest {
            id: Uuid::new_v4(),
            filename: filename.to_string(),
            total_size: total,
            chunk_size,
            total_chunks: total.div_ceil(chunk_size as u64) as u32,
            sha256,
        }
    }

    fn with_source(manifest: TransferManifest, source: Source) -> Self {
        Self {
            manifest,
            source,
            next_unsent: 0,
            inflight: HashMap::new(),
            acked: HashSet::new(),
            last_progress: Instant::now(),
        }
    }

    fn read_chunk(&mut self, id: u32) -> Result<Bytes, TransferError> {
        let start = id as u64 * self.manifest.chunk_size as u64;
        let len = self.manifest.chunk_len(id);
        match &mut self.source {
            Source::Memory(data) => Ok(Bytes::copy_from_slice(&data[start as usize..start as usize + len])),
            Source::File(file) => {
                let mut buf = vec![0u8; len];
                file.seek(SeekFrom::Start(start))?;
                file.read_exact(&mut buf)?;
                Ok(Bytes::from(buf))
            }
        }
    }

    /// Chunks to send now: overdue resends first, then new chunks while fewer than `window`
    /// are in flight.
    pub fn next_chunks(&mut self, window: usize, resend_after: Duration) -> Result<Vec<(u32, Bytes)>, TransferError> {
        let now = Instant::now();
        let mut ids: Vec<u32> = self
            .inflight
            .iter()
            .filter(|(_, sent)| now.duration_since(**sent) >= resend_after)
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        // Overdue ids are already in `inflight`; only new chunks grow the window.
        let mut added = 0;
        while self.inflight.len() + added < window && self.next_unsent < self.manifest.total_chunks {
            let id = self.next_unsent;
            self.next_unsent += 1;
            if !self.acked.contains(&id) {
                ids.push(id);
                added += 1;
            }
        }
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let data = self.read_chunk(id)?;
            self.inflight.insert(id, now);
            out.push((id, data));
        }
        Ok(out)
    }

    /// Process a selective ACK.
    pub fn on_ack(&mut self, ack: &ChunkAck) {
        for &id in &ack.received_chunks {
            if id < self.manifest.total_chunks && self.acked.insert(id) {
                self.last_progress = Instant::now();
            }
            self.inflight.remove(&id);
        }
    }

    /// Time since the last newly-acknowledged chunk (stall detection).
    pub fn idle_for(&self) -> Duration {
        self.last_progress.elapsed()
    }

    pub fn missing_chunks(&self) -> Vec<u32> {
        (0..self.manifest.total_chunks)
            .filter(|id| !self.acked.contains(id))
            .collect()
    }

    pub fn is_complete(&self) -> bool {
        self.acked.len() == self.manifest.total_chunks as usize
    }

    pub fn progress(&self) -> f64 {
        if self.manifest.total_chunks == 0 {
            return 1.0;
        }
        self.acked.len() as f64 / self.manifest.total_chunks as f64
    }
}

/// Receiver side of a chunked transfer, backed by a `.part` file.
pub struct ChunkedReceiver {
    pub manifest: TransferManifest,
    file: File,
    part_path: PathBuf,
    final_path: PathBuf,
    received: HashSet<u32>,
}

impl ChunkedReceiver {
    /// Validate the offer and create `dir/.{storage_name}.part`; the finished file becomes
    /// `dir/{storage_name}` (with a numeric suffix if that name is taken).
    pub fn create(manifest: TransferManifest, dir: &Path, storage_name: &str) -> Result<Self, TransferError> {
        manifest.validate()?;
        std::fs::create_dir_all(dir)?;
        let storage_name = sanitize_filename(storage_name);
        let part_path = dir.join(format!(".{storage_name}.part"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&part_path)?;
        file.set_len(manifest.total_size)?;
        Ok(Self {
            final_path: dir.join(&storage_name),
            manifest,
            file,
            part_path,
            received: HashSet::new(),
        })
    }

    /// Store one chunk. Returns false if the chunk id or length does not match the manifest.
    pub fn receive_chunk(&mut self, chunk_id: u32, data: &[u8]) -> Result<bool, TransferError> {
        if chunk_id >= self.manifest.total_chunks || data.len() != self.manifest.chunk_len(chunk_id) {
            return Ok(false);
        }
        if self.received.contains(&chunk_id) {
            return Ok(true);
        }
        self.file
            .seek(SeekFrom::Start(chunk_id as u64 * self.manifest.chunk_size as u64))?;
        self.file.write_all(data)?;
        self.received.insert(chunk_id);
        Ok(true)
    }

    pub fn ack_for(&self, chunk_id: u32) -> ChunkAck {
        ChunkAck {
            transfer_id: self.manifest.id,
            received_chunks: vec![chunk_id],
            missing_chunks: Vec::new(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.received.len() == self.manifest.total_chunks as usize
    }

    /// Verify SHA-256 and move the file into place. Returns the final path.
    pub fn finalize(mut self) -> Result<PathBuf, TransferError> {
        if !self.is_complete() {
            return Err(TransferError::Incomplete);
        }
        self.file.flush()?;
        self.file.sync_all()?;
        self.file.seek(SeekFrom::Start(0))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        if hex::encode(hasher.finalize()) != self.manifest.sha256.to_lowercase() {
            let _ = std::fs::remove_file(&self.part_path);
            return Err(TransferError::ChecksumMismatch);
        }
        let dest = unique_path(&self.final_path);
        std::fs::rename(&self.part_path, &dest)?;
        Ok(dest)
    }

    /// Delete the partial file (cancel / peer lost).
    pub fn discard(self) {
        let _ = std::fs::remove_file(&self.part_path);
    }

    pub fn progress(&self) -> f64 {
        if self.manifest.total_chunks == 0 {
            return 1.0;
        }
        self.received.len() as f64 / self.manifest.total_chunks as f64
    }
}

fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let parent = path.parent().unwrap_or(Path::new("."));
    (1..)
        .map(|n| parent.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("unbounded search")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: Vec<u8>, chunk: usize) {
        let dir = tempfile::tempdir().unwrap();
        let mut tx = ChunkedSender::from_bytes("a.bin", data.clone(), chunk);
        let mut rx = ChunkedReceiver::create(tx.manifest.clone(), dir.path(), "a.bin").unwrap();
        // Lose every 3rd chunk on first send to exercise resends.
        let mut first = true;
        while !tx.is_complete() {
            let batch = tx.next_chunks(8, Duration::ZERO).unwrap();
            for (i, (id, bytes)) in batch.into_iter().enumerate() {
                if first && i % 3 == 0 {
                    continue;
                }
                assert!(rx.receive_chunk(id, &bytes).unwrap());
                tx.on_ack(&rx.ack_for(id));
            }
            first = false;
        }
        assert!(rx.is_complete());
        let path = rx.finalize().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
    }

    #[test]
    fn transfer_roundtrip_various_sizes() {
        roundtrip(vec![], 4000);
        roundtrip(vec![7; 1], 4000);
        roundtrip((0..4000u32).map(|i| i as u8).collect(), 4000);
        roundtrip((0..100_001u32).map(|i| (i % 251) as u8).collect(), 4000);
    }

    #[test]
    fn window_limits_inflight_and_resends_only_when_due() {
        let mut tx = ChunkedSender::from_bytes("f", vec![1; 10 * 100], 100);
        assert_eq!(tx.next_chunks(4, Duration::from_secs(60)).unwrap().len(), 4);
        assert!(tx.next_chunks(4, Duration::from_secs(60)).unwrap().is_empty(), "window full");
        tx.on_ack(&ChunkAck { transfer_id: tx.manifest.id, received_chunks: vec![0], missing_chunks: vec![] });
        let next = tx.next_chunks(4, Duration::from_secs(60)).unwrap();
        assert_eq!(next.iter().map(|c| c.0).collect::<Vec<_>>(), vec![4]);
        let resend = tx.next_chunks(4, Duration::ZERO).unwrap();
        assert_eq!(resend.len(), 4, "overdue chunks are resent");
    }

    #[test]
    fn corrupted_chunk_fails_checksum() {
        let dir = tempfile::tempdir().unwrap();
        let mut tx = ChunkedSender::from_bytes("x", vec![5; 300], 100);
        let mut rx = ChunkedReceiver::create(tx.manifest.clone(), dir.path(), "x").unwrap();
        for (id, mut bytes) in tx.next_chunks(8, Duration::ZERO).unwrap() {
            if id == 1 {
                bytes = Bytes::from(vec![6; 100]);
            }
            rx.receive_chunk(id, &bytes).unwrap();
        }
        assert!(matches!(rx.finalize(), Err(TransferError::ChecksumMismatch)));
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "part file removed");
    }

    #[test]
    fn rejects_hostile_offers_and_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = ChunkedSender::from_bytes("x", vec![0; 1000], 100).manifest;
        m.total_chunks = u32::MAX;
        assert!(ChunkedReceiver::create(m.clone(), dir.path(), "x").is_err());
        m.total_chunks = 10;
        m.chunk_size = 0;
        assert!(ChunkedReceiver::create(m.clone(), dir.path(), "x").is_err());
        m.chunk_size = 100;
        let mut rx = ChunkedReceiver::create(m, dir.path(), "x").unwrap();
        assert!(!rx.receive_chunk(10, &[0; 100]).unwrap(), "out of range");
        assert!(!rx.receive_chunk(0, &[0; 99]).unwrap(), "wrong length");
    }

    #[test]
    fn filenames_cannot_escape_receive_dir() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("..\\..\\boot.ini"), "boot.ini");
        assert_eq!(sanitize_filename(".."), "file");
        assert_eq!(sanitize_filename(""), "file");
        assert_eq!(sanitize_filename("a/b/"), "file");
        assert_eq!(sanitize_filename("ok name.png"), "ok name.png");
        assert_eq!(sanitize_filename("evil\u{0}\n.txt"), "evil.txt");
        assert!(sanitize_filename(&"é".repeat(300)).len() <= 200);
    }

    #[test]
    fn duplicate_final_names_get_suffix() {
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..2 {
            let mut tx = ChunkedSender::from_bytes("same.txt", b"hi".to_vec(), 100);
            let mut rx = ChunkedReceiver::create(tx.manifest.clone(), dir.path(), "same.txt").unwrap();
            for (id, b) in tx.next_chunks(8, Duration::ZERO).unwrap() {
                rx.receive_chunk(id, &b).unwrap();
            }
            rx.finalize().unwrap();
        }
        assert!(dir.path().join("same.txt").exists());
        assert!(dir.path().join("same (1).txt").exists());
    }
}
