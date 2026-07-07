//! COBS framing with CRC32 integrity checking.
//!
//! Wire format (before COBS encoding):
//! ```text
//! [magic:2][flags:1][seq:2][ack:2][len:2][payload:0..MAX][crc32:4]
//! ```
//! After COBS encoding, frames are delimited by 0x00.

use bytes::{BufMut, Bytes, BytesMut};
use crc32fast::Hasher;
use thiserror::Error;

pub const FRAME_MAGIC: [u8; 2] = [0x53, 0x52]; // "SR"
pub const HEADER_SIZE: usize = 9;
pub const CRC_SIZE: usize = 4;
pub const MAX_PAYLOAD_SIZE: usize = 4096;
pub const MAX_FRAME_SIZE: usize = HEADER_SIZE + MAX_PAYLOAD_SIZE + CRC_SIZE;

/// Frame type flags (bitfield).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFlags {
    Data = 0x01,
    Ack = 0x02,
    Nack = 0x04,
    Control = 0x08,
    Fin = 0x10,
    Chunk = 0x20,
}

impl FrameFlags {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x02 => FrameFlags::Ack,
            0x04 => FrameFlags::Nack,
            0x08 => FrameFlags::Control,
            0x10 => FrameFlags::Fin,
            0x20 => FrameFlags::Chunk,
            _ => FrameFlags::Data,
        }
    }

    pub fn to_byte(self) -> u8 {
        self as u8
    }

    pub fn is_ack_only(self) -> bool {
        matches!(self, FrameFlags::Ack | FrameFlags::Nack)
    }
}

/// Parsed serial frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub flags: FrameFlags,
    pub seq: u16,
    pub ack: u16,
    pub payload: Bytes,
}

impl Frame {
    pub fn data(seq: u16, ack: u16, payload: impl Into<Bytes>) -> Self {
        Self {
            flags: FrameFlags::Data,
            seq,
            ack,
            payload: payload.into(),
        }
    }

    pub fn ack(seq: u16, ack: u16) -> Self {
        Self {
            flags: FrameFlags::Ack,
            seq,
            ack,
            payload: Bytes::new(),
        }
    }

    pub fn nack(seq: u16, ack: u16, missing_seq: u16) -> Self {
        let mut payload = BytesMut::with_capacity(2);
        payload.put_u16(missing_seq);
        Self {
            flags: FrameFlags::Nack,
            seq,
            ack,
            payload: payload.freeze(),
        }
    }

    pub fn chunk(seq: u16, ack: u16, chunk_id: u32, data: impl Into<Bytes>) -> Self {
        let data = data.into();
        let mut payload = BytesMut::with_capacity(4 + data.len());
        payload.put_u32(chunk_id);
        payload.extend_from_slice(&data);
        Self {
            flags: FrameFlags::Chunk,
            seq,
            ack,
            payload: payload.freeze(),
        }
    }

    pub fn control(seq: u16, ack: u16, payload: impl Into<Bytes>) -> Self {
        Self {
            flags: FrameFlags::Control,
            seq,
            ack,
            payload: payload.into(),
        }
    }

    pub fn fin(seq: u16, ack: u16) -> Self {
        Self {
            flags: FrameFlags::Fin,
            seq,
            ack,
            payload: Bytes::new(),
        }
    }

    /// Serialize to COBS-encoded bytes with 0x00 delimiter.
    pub fn encode(&self) -> Vec<u8> {
        let raw = self.serialize_raw();
        let mut encoded = vec![0u8; cobs::max_encoding_length(raw.len())];
        let len = cobs::encode(&raw, &mut encoded);
        encoded.truncate(len);
        encoded.push(0x00); // frame delimiter
        encoded
    }

    fn serialize_raw(&self) -> Vec<u8> {
        let payload_len = self.payload.len();
        assert!(payload_len <= MAX_PAYLOAD_SIZE);

        let mut buf = BytesMut::with_capacity(HEADER_SIZE + payload_len + CRC_SIZE);
        buf.put_slice(&FRAME_MAGIC);
        buf.put_u8(self.flags.to_byte());
        buf.put_u16(self.seq);
        buf.put_u16(self.ack);
        buf.put_u16(payload_len as u16);
        buf.extend_from_slice(&self.payload);

        let crc = compute_crc(&buf);
        buf.put_u32(crc);
        buf.to_vec()
    }

    /// Decode a COBS frame (without trailing 0x00).
    pub fn decode(cobs_data: &[u8]) -> Result<Self, FrameError> {
        let mut raw = vec![0u8; cobs_data.len()];
        let len = cobs::decode(cobs_data, &mut raw).map_err(|_| FrameError::CobsDecode)?;
        raw.truncate(len);
        Self::parse_raw(&raw)
    }

    fn parse_raw(raw: &[u8]) -> Result<Self, FrameError> {
        if raw.len() < HEADER_SIZE + CRC_SIZE {
            return Err(FrameError::TooShort);
        }

        if raw[0..2] != FRAME_MAGIC {
            return Err(FrameError::BadMagic);
        }

        let payload_len = u16::from_be_bytes([raw[7], raw[8]]) as usize;
        let expected = HEADER_SIZE + payload_len + CRC_SIZE;
        if raw.len() != expected {
            return Err(FrameError::LengthMismatch);
        }

        let crc_offset = HEADER_SIZE + payload_len;
        let stored_crc = u32::from_be_bytes([
            raw[crc_offset],
            raw[crc_offset + 1],
            raw[crc_offset + 2],
            raw[crc_offset + 3],
        ]);
        let computed = compute_crc(&raw[..crc_offset]);
        if stored_crc != computed {
            return Err(FrameError::CrcMismatch);
        }

        Ok(Frame {
            flags: FrameFlags::from_byte(raw[2]),
            seq: u16::from_be_bytes([raw[3], raw[4]]),
            ack: u16::from_be_bytes([raw[5], raw[6]]),
            payload: Bytes::copy_from_slice(&raw[HEADER_SIZE..crc_offset]),
        })
    }
}

fn compute_crc(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("COBS decode failed")]
    CobsDecode,
    #[error("frame too short")]
    TooShort,
    #[error("invalid magic bytes")]
    BadMagic,
    #[error("length mismatch")]
    LengthMismatch,
    #[error("CRC32 mismatch")]
    CrcMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_data_frame() {
        let frame = Frame::data(42, 17, Bytes::from_static(b"hello serial"));
        let encoded = frame.encode();
        let decoded = Frame::decode(&encoded[..encoded.len() - 1]).unwrap();
        assert_eq!(decoded.flags, FrameFlags::Data);
        assert_eq!(decoded.seq, 42);
        assert_eq!(decoded.ack, 17);
        assert_eq!(decoded.payload, b"hello serial"[..]);
    }

    #[test]
    fn ack_frame_is_small() {
        let frame = Frame::ack(0, 99);
        let encoded = frame.encode();
        // Target: under 16 bytes for ACK frames
        assert!(encoded.len() <= 16, "ACK frame size: {}", encoded.len());
    }

    #[test]
    fn crc_detects_corruption() {
        let frame = Frame::data(1, 0, Bytes::from_static(b"test"));
        let mut encoded = frame.encode();
        if encoded.len() > 5 {
            encoded[3] ^= 0xFF;
        }
        let result = Frame::decode(&encoded[..encoded.len() - 1]);
        assert!(matches!(result, Err(FrameError::CrcMismatch)));
    }
}