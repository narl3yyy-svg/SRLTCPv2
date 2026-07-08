//! zstd compression for folder/file bulk transfer (streaming-friendly).

use std::io::{Read, Write};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompressError {
    #[error("zstd error: {0}")]
    Zstd(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Compress bytes with default level (3 — fast, good ratio on low-end HW).
pub fn compress(data: &[u8]) -> Result<Vec<u8>, CompressError> {
    zstd::encode_all(data, 3).map_err(|e| CompressError::Zstd(e.to_string()))
}

/// Decompress zstd payload.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, CompressError> {
    zstd::decode_all(data).map_err(|e| CompressError::Zstd(e.to_string()))
}

/// Stream-compress from reader to writer.
pub fn compress_stream<R: Read, W: Write>(mut input: R, mut output: W) -> Result<u64, CompressError> {
    let mut encoder = zstd::stream::Encoder::new(&mut output, 3)
        .map_err(|e| CompressError::Zstd(e.to_string()))?;
    let n = std::io::copy(&mut input, &mut encoder)?;
    encoder.finish().map_err(|e| CompressError::Zstd(e.to_string()))?;
    Ok(n)
}