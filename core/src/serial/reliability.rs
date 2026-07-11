//! Sequenced reliability layer with ACK/NACK and timeout-based retransmission.
//!
//! Designed for noisy serial links: small control packets, selective retransmit,
//! and efficient bandwidth usage via cumulative ACK piggybacking.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use bytes::Bytes;
use tracing::{debug, trace, warn};

use super::frame::{Frame, FrameFlags, MAX_PAYLOAD_SIZE};

/// Default retransmission timeout for serial links.
pub const DEFAULT_RTO_MS: u64 = 200;
/// Maximum unacknowledged frames in flight.
pub const DEFAULT_WINDOW_SIZE: usize = 8;
/// Maximum retransmission attempts before giving up.
pub const MAX_RETRIES: u32 = 12;
/// Cap out-of-order receive buffer to resist memory DoS on noisy/malicious links.
pub const MAX_RECV_BUFFER: usize = 64;

/// Sliding-window reliability engine.
pub struct ReliabilityLayer {
    /// Next sequence number to assign (sender side).
    send_seq: u16,
    /// Highest contiguous sequence received (receiver side).
    recv_next: u16,
    /// Cumulative ACK to piggyback on outbound frames.
    send_ack: u16,
    /// Unacknowledged sent frames awaiting ACK.
    inflight: HashMap<u16, InflightEntry>,
    /// Out-of-order received frames buffered until gap fills.
    recv_buffer: HashMap<u16, Bytes>,
    /// Ordered delivery queue.
    deliver_queue: VecDeque<Bytes>,
    /// Retransmission timeout.
    rto: Duration,
    /// Sliding window size.
    window_size: usize,
    /// Duplicate ACK counter for fast retransmit.
    dup_ack_count: u32,
    last_ack: u16,
}

struct InflightEntry {
    frame: Frame,
    sent_at: Instant,
    retries: u32,
}

impl ReliabilityLayer {
    pub fn new() -> Self {
        Self {
            send_seq: 0,
            recv_next: 0,
            send_ack: 0,
            inflight: HashMap::new(),
            recv_buffer: HashMap::new(),
            deliver_queue: VecDeque::new(),
            rto: Duration::from_millis(DEFAULT_RTO_MS),
            window_size: DEFAULT_WINDOW_SIZE,
            dup_ack_count: 0,
            last_ack: 0,
        }
    }

    pub fn with_rto(mut self, rto: Duration) -> Self {
        self.rto = rto;
        self
    }

    pub fn with_window(mut self, size: usize) -> Self {
        self.window_size = size;
        self
    }

    /// Prepare a DATA frame for sending if window has space.
    pub fn prepare_send(&mut self, payload: Bytes) -> Option<Frame> {
        if self.inflight.len() >= self.window_size {
            return None;
        }
        if payload.len() > MAX_PAYLOAD_SIZE {
            warn!(len = payload.len(), "payload exceeds max, must chunk");
            return None;
        }

        let seq = self.send_seq;
        self.send_seq = seq.wrapping_add(1);

        let frame = Frame::data(seq, self.send_ack, payload);
        self.inflight.insert(
            seq,
            InflightEntry {
                frame: frame.clone(),
                sent_at: Instant::now(),
                retries: 0,
            },
        );
        Some(frame)
    }

    /// Prepare a CHUNK frame for file transfer.
    pub fn prepare_chunk_send(&mut self, chunk_id: u32, data: Bytes) -> Option<Frame> {
        if self.inflight.len() >= self.window_size {
            return None;
        }

        let seq = self.send_seq;
        self.send_seq = seq.wrapping_add(1);

        let frame = Frame::chunk(seq, self.send_ack, chunk_id, data);
        self.inflight.insert(
            seq,
            InflightEntry {
                frame: frame.clone(),
                sent_at: Instant::now(),
                retries: 0,
            },
        );
        Some(frame)
    }

    /// Process an inbound frame; returns ACK/NACK responses and any deliverable data.
    pub fn on_receive(&mut self, frame: &Frame) -> ReceiveResult {
        let mut responses = Vec::new();

        match frame.flags {
            FrameFlags::Ack => {
                self.handle_ack(frame.ack);
            }
            FrameFlags::Nack => {
                if frame.payload.len() >= 2 {
                    let missing = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
                    self.handle_nack(missing);
                }
            }
            FrameFlags::Data | FrameFlags::Chunk | FrameFlags::Control => {
                if frame.seq == self.recv_next {
                    self.deliver_queue.push_back(frame.payload.clone());
                    self.recv_next = self.recv_next.wrapping_add(1);

                    // Drain buffered out-of-order frames
                    while let Some(buf) = self.recv_buffer.remove(&self.recv_next) {
                        self.deliver_queue.push_back(buf);
                        self.recv_next = self.recv_next.wrapping_add(1);
                    }
                } else if seq_gt(frame.seq, self.recv_next) {
                    // Out of order — buffer with hard cap (DoS resistance)
                    if self.recv_buffer.len() < MAX_RECV_BUFFER
                        || self.recv_buffer.contains_key(&frame.seq)
                    {
                        self.recv_buffer
                            .entry(frame.seq)
                            .or_insert_with(|| frame.payload.clone());
                    } else {
                        warn!(
                            seq = frame.seq,
                            buffered = self.recv_buffer.len(),
                            "recv_buffer full — dropping out-of-order frame"
                        );
                    }
                    // Send NACK for the missing sequence
                    responses.push(Frame::nack(0, self.recv_next, self.recv_next));
                }
                // Duplicate or old seq — silently drop, still ACK
            }
            FrameFlags::Fin => {
                trace!(seq = frame.seq, "received FIN");
            }
        }

        // Piggyback ACK for any data/control frame received
        if !frame.flags.is_ack_only() {
            self.send_ack = self.recv_next;
            responses.push(Frame::ack(0, self.recv_next));
        }

        // Process piggybacked ACK on inbound data frames
        if !frame.flags.is_ack_only() && frame.ack != 0 {
            self.handle_ack(frame.ack);
        }

        let deliver: Vec<Bytes> = self.deliver_queue.drain(..).collect();
        ReceiveResult {
            ack_responses: responses,
            delivered: deliver,
        }
    }

    fn handle_ack(&mut self, ack: u16) {
        if ack == self.last_ack {
            self.dup_ack_count += 1;
            if self.dup_ack_count >= 3 {
                // Fast retransmit: resend earliest inflight
                if let Some(&seq) = self.inflight.keys().min() {
                    debug!(seq, "fast retransmit on triple dup ACK");
                    if let Some(entry) = self.inflight.get_mut(&seq) {
                        entry.sent_at = Instant::now() - self.rto;
                        entry.retries += 1;
                    }
                }
                self.dup_ack_count = 0;
            }
        } else {
            self.dup_ack_count = 0;
            self.last_ack = ack;
        }

        // Remove acknowledged frames
        let to_remove: Vec<u16> = self
            .inflight
            .keys()
            .filter(|&&seq| seq_lt(seq, ack))
            .copied()
            .collect();
        for seq in to_remove {
            self.inflight.remove(&seq);
            trace!(seq, "frame ACKed");
        }
    }

    fn handle_nack(&mut self, missing_seq: u16) {
        if let Some(entry) = self.inflight.get_mut(&missing_seq) {
            debug!(seq = missing_seq, "NACK-triggered retransmit");
            entry.sent_at = Instant::now() - self.rto;
            entry.retries += 1;
        }
    }

    /// Collect frames that need retransmission due to timeout.
    pub fn poll_retransmits(&mut self) -> Vec<Frame> {
        let now = Instant::now();
        let mut retransmits = Vec::new();

        for (seq, entry) in self.inflight.iter_mut() {
            if now.duration_since(entry.sent_at) >= self.rto {
                if entry.retries >= MAX_RETRIES {
                    warn!(seq, "max retries exceeded, dropping frame");
                    continue;
                }
                entry.sent_at = now;
                entry.retries += 1;
                debug!(seq, retries = entry.retries, "timeout retransmit");
                retransmits.push(entry.frame.clone());
            }
        }

        // Remove expired entries
        self.inflight
            .retain(|_, e| e.retries < MAX_RETRIES);

        retransmits
    }

    pub fn pending_count(&self) -> usize {
        self.inflight.len()
    }

    pub fn create_fin(&mut self) -> Frame {
        Frame::fin(self.send_seq, self.send_ack)
    }
}

/// Result of processing an inbound frame.
pub struct ReceiveResult {
    pub ack_responses: Vec<Frame>,
    pub delivered: Vec<Bytes>,
}

/// Sequence comparison with wrap-around (u16 ring).
fn seq_lt(a: u16, b: u16) -> bool {
    ((b.wrapping_sub(a)) as u32) < 0x8000
}

fn seq_gt(a: u16, b: u16) -> bool {
    seq_lt(b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_order_delivery() {
        let mut rl = ReliabilityLayer::new();

        let f1 = Frame::data(0, 0, Bytes::from_static(b"msg1"));
        let r1 = rl.on_receive(&f1);
        assert_eq!(r1.delivered.len(), 1);
        assert_eq!(&r1.delivered[0][..], b"msg1");

        let f2 = Frame::data(1, 0, Bytes::from_static(b"msg2"));
        let r2 = rl.on_receive(&f2);
        assert_eq!(r2.delivered.len(), 1);
    }

    #[test]
    fn out_of_order_buffered() {
        let mut rl = ReliabilityLayer::new();

        // Receive seq 1 before seq 0
        let f1 = Frame::data(1, 0, Bytes::from_static(b"second"));
        let r1 = rl.on_receive(&f1);
        assert!(r1.delivered.is_empty());
        assert!(!r1.ack_responses.is_empty());

        let f0 = Frame::data(0, 0, Bytes::from_static(b"first"));
        let r0 = rl.on_receive(&f0);
        assert_eq!(r0.delivered.len(), 2);
        assert_eq!(&r0.delivered[0][..], b"first");
        assert_eq!(&r0.delivered[1][..], b"second");
    }

    #[test]
    fn send_window_limits_inflight() {
        let mut rl = ReliabilityLayer::new().with_window(2);

        assert!(rl.prepare_send(Bytes::from_static(b"a")).is_some());
        assert!(rl.prepare_send(Bytes::from_static(b"b")).is_some());
        assert!(rl.prepare_send(Bytes::from_static(b"c")).is_none());
        assert_eq!(rl.pending_count(), 2);
    }
}