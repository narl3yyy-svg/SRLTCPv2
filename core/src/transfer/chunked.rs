//! Resumable chunked file/folder transfer with selective ACKs.

use std::collections::HashSet;
use std::path::Path;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tracing::{debug, info};
use uuid::Uuid;



/// Default chunk size: 4KB (fits in serial frame with overhead).
pub const DEFAULT_CHUNK_SIZE: usize = 4000;

#[derive(Debug, Error)]
pub enum TransferError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("transfer not found: {0}")]
    NotFound(Uuid),
    #[error("checksum mismatch for chunk {0}")]
    ChecksumMismatch(u32),
    #[error("transfer complete")]
    Complete,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkAck {
    pub transfer_id: Uuid,
    pub received_chunks: Vec<u32>,
    pub missing_chunks: Vec<u32>,
}

/// Sender side of a chunked transfer.
pub struct ChunkedSender {
    pub manifest: TransferManifest,
    data: Vec<u8>,
    sent_chunks: HashSet<u32>,
    acked_chunks: HashSet<u32>,
}

impl ChunkedSender {
    pub fn from_file(path: &Path, chunk_size: usize) -> Result<Self, TransferError> {
        let data = std::fs::read(path)?;
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let sha256 = hex::encode(Sha256::digest(&data));
        let total_chunks = ((data.len() + chunk_size - 1) / chunk_size) as u32;

        let manifest = TransferManifest {
            id: Uuid::new_v4(),
            filename,
            total_size: data.len() as u64,
            chunk_size,
            total_chunks,
            sha256,
        };

        info!(
            id = %manifest.id,
            file = %manifest.filename,
            chunks = total_chunks,
            "transfer created"
        );

        Ok(Self {
            manifest,
            data,
            sent_chunks: HashSet::new(),
            acked_chunks: HashSet::new(),
        })
    }

    pub fn from_bytes(filename: &str, data: Vec<u8>, chunk_size: usize) -> Self {
        let sha256 = hex::encode(Sha256::digest(&data));
        let total_chunks = ((data.len() + chunk_size - 1) / chunk_size) as u32;

        Self {
            manifest: TransferManifest {
                id: Uuid::new_v4(),
                filename: filename.to_string(),
                total_size: data.len() as u64,
                chunk_size,
                total_chunks,
                sha256,
            },
            data,
            sent_chunks: HashSet::new(),
            acked_chunks: HashSet::new(),
        }
    }

    /// Get next chunk(s) to send, respecting reliability window.
    pub fn next_chunks(&mut self, max: usize) -> Vec<(u32, Bytes)> {
        let mut chunks = Vec::new();
        for id in 0..self.manifest.total_chunks {
            if self.acked_chunks.contains(&id) {
                continue;
            }
            let start = (id as usize) * self.manifest.chunk_size;
            let end = ((id as usize + 1) * self.manifest.chunk_size).min(self.data.len());
            let chunk_data = Bytes::copy_from_slice(&self.data[start..end]);
            self.sent_chunks.insert(id);
            chunks.push((id, chunk_data));
            if chunks.len() >= max {
                break;
            }
        }
        chunks
    }

    /// Process selective ACK — only retransmit missing chunks.
    pub fn on_ack(&mut self, ack: &ChunkAck) {
        for &id in &ack.received_chunks {
            self.acked_chunks.insert(id);
            debug!(chunk = id, "chunk ACKed");
        }
    }

    pub fn missing_chunks(&self) -> Vec<u32> {
        (0..self.manifest.total_chunks)
            .filter(|id| !self.acked_chunks.contains(id))
            .collect()
    }

    pub fn is_complete(&self) -> bool {
        self.acked_chunks.len() == self.manifest.total_chunks as usize
    }

    pub fn progress(&self) -> f64 {
        if self.manifest.total_chunks == 0 {
            return 1.0;
        }
        self.acked_chunks.len() as f64 / self.manifest.total_chunks as f64
    }
}

/// Receiver side of a chunked transfer.
pub struct ChunkedReceiver {
    pub manifest: TransferManifest,
    chunks: Vec<Option<Bytes>>,
    received: HashSet<u32>,
}

impl ChunkedReceiver {
    pub fn new(manifest: TransferManifest) -> Self {
        let total = manifest.total_chunks as usize;
        Self {
            manifest,
            chunks: vec![None; total],
            received: HashSet::new(),
        }
    }

    pub fn receive_chunk(&mut self, chunk_id: u32, data: Bytes) -> Result<(), TransferError> {
        let idx = chunk_id as usize;
        if idx >= self.chunks.len() {
            return Ok(());
        }
        self.chunks[idx] = Some(data);
        self.received.insert(chunk_id);
        Ok(())
    }

    pub fn selective_ack(&self) -> ChunkAck {
        let missing: Vec<u32> = (0..self.manifest.total_chunks)
            .filter(|id| !self.received.contains(id))
            .collect();
        ChunkAck {
            transfer_id: self.manifest.id,
            received_chunks: self.received.iter().copied().collect(),
            missing_chunks: missing,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.received.len() == self.manifest.total_chunks as usize
    }

    pub fn assemble(&self) -> Result<Vec<u8>, TransferError> {
        if !self.is_complete() {
            return Err(TransferError::NotFound(self.manifest.id));
        }
        let mut data = Vec::with_capacity(self.manifest.total_size as usize);
        for chunk in &self.chunks {
            if let Some(bytes) = chunk {
                data.extend_from_slice(bytes);
            }
        }

        let hash = hex::encode(Sha256::digest(&data));
        if hash != self.manifest.sha256 {
            return Err(TransferError::ChecksumMismatch(0));
        }
        Ok(data)
    }

    pub fn progress(&self) -> f64 {
        if self.manifest.total_chunks == 0 {
            return 1.0;
        }
        self.received.len() as f64 / self.manifest.total_chunks as f64
    }
}