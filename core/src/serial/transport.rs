//! Async serial port transport with COBS framing and reliability layer.
//!
//! One link task owns the [`ReliabilityLayer`]: it fragments outbound messages into
//! frames, feeds inbound bytes from a blocking reader thread through COBS/CRC decoding,
//! sends ACKs, retransmits on timeout and reassembles messages. A writer thread does the
//! blocking port writes. Dropping the transport (or [`SerialTransport::shutdown`]) stops all
//! three and closes the port.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::frame::{Frame, MAX_PAYLOAD_SIZE};
use super::reliability::ReliabilityLayer;

/// Largest reassembled message accepted from the link.
pub const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
const FRAG_MORE: u8 = 0x01;
const FRAG_LAST: u8 = 0x00;
const FRAGMENT_DATA: usize = MAX_PAYLOAD_SIZE - 1;

/// Serial transport configuration.
#[derive(Debug, Clone)]
pub struct SerialConfig {
    pub port_name: String,
    pub baud_rate: u32,
    pub rto_ms: u64,
    pub window_size: usize,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port_name: "/dev/ttyUSB0".to_string(),
            baud_rate: 115200,
            rto_ms: 400,
            window_size: 8,
        }
    }
}

impl SerialConfig {
    /// Retransmission timeout scaled to the line rate: a full window of max-size frames must
    /// fit on the wire before we consider a frame lost.
    pub fn effective_rto(&self) -> Duration {
        let bytes_per_sec = (self.baud_rate.max(1200) / 10) as u64;
        let window_ms = (self.window_size as u64 * (MAX_PAYLOAD_SIZE as u64 + 32) * 1000) / bytes_per_sec;
        Duration::from_millis(self.rto_ms.max(window_ms + 200))
    }
}

/// Events emitted by the serial transport.
#[derive(Debug, Clone)]
pub enum SerialEvent {
    Connected { port: String },
    Disconnected { port: String, reason: String },
    DataReceived(Bytes),
    Error(String),
}

/// Managed serial transport with graceful shutdown.
pub struct SerialTransport {
    config: SerialConfig,
    running: Arc<AtomicBool>,
    event_tx: mpsc::Sender<SerialEvent>,
    outbound_tx: std::sync::Mutex<Option<mpsc::Sender<Bytes>>>,
}

impl SerialTransport {
    pub fn new(config: SerialConfig) -> (Self, mpsc::Receiver<SerialEvent>) {
        let (event_tx, event_rx) = mpsc::channel(256);
        let transport = Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            event_tx,
            outbound_tx: std::sync::Mutex::new(None),
        };
        (transport, event_rx)
    }

    pub fn port_name(&self) -> &str {
        &self.config.port_name
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub async fn start(&self) -> Result<(), String> {
        #[cfg(not(all(not(target_os = "android"), feature = "desktop")))]
        {
            return Err("serial transport is not available on this platform".to_string());
        }

        #[cfg(all(not(target_os = "android"), feature = "desktop"))]
        {
            use std::io::{Read, Write};

            if self.running.load(Ordering::SeqCst) {
                return Ok(());
            }
            let mut reader_port = serialport::new(&self.config.port_name, self.config.baud_rate)
                .timeout(Duration::from_millis(100))
                .open()
                .map_err(|e| format!("failed to open {}: {e}", self.config.port_name))?;
            let mut writer_port = reader_port
                .try_clone()
                .map_err(|e| format!("failed to clone {}: {e}", self.config.port_name))?;
            info!(port = %self.config.port_name, baud = self.config.baud_rate, "serial port opened");
            self.running.store(true, Ordering::SeqCst);

            // Reader thread: raw bytes → link task.
            let (raw_tx, raw_rx) = mpsc::channel::<Vec<u8>>(256);
            let running_r = self.running.clone();
            let err_tx = self.event_tx.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while running_r.load(Ordering::SeqCst) {
                    match reader_port.read(&mut buf) {
                        Ok(0) => continue,
                        Ok(n) => {
                            if raw_tx.blocking_send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut
                            || e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => {
                            let _ = err_tx.blocking_send(SerialEvent::Error(format!("serial read error: {e}")));
                            break;
                        }
                    }
                }
            });

            // Writer thread: encoded frames → port. Ends when the link task drops its sender.
            let (wire_tx, wire_rx) = std::sync::mpsc::channel::<Vec<u8>>();
            std::thread::spawn(move || {
                for bytes in wire_rx {
                    if let Err(e) = writer_port.write_all(&bytes).and_then(|_| writer_port.flush()) {
                        warn!(error = %e, "serial write failed");
                        break;
                    }
                }
            });

            let (outbound_tx, outbound_rx) = mpsc::channel::<Bytes>(64);
            *self.outbound_tx.lock().expect("serial lock") = Some(outbound_tx);

            let link = LinkConfig {
                port: self.config.port_name.clone(),
                rto: self.config.effective_rto(),
                window: self.config.window_size,
            };
            let running = self.running.clone();
            let event_tx = self.event_tx.clone();
            tokio::spawn(async move {
                let reason = run_link(link.clone(), raw_rx, outbound_rx, wire_tx, event_tx.clone(), running.clone()).await;
                running.store(false, Ordering::SeqCst);
                let _ = event_tx.send(SerialEvent::Disconnected { port: link.port, reason }).await;
            });

            let _ = self
                .event_tx
                .send(SerialEvent::Connected { port: self.config.port_name.clone() })
                .await;
            Ok(())
        }
    }

    /// Queue one message (any size up to [`MAX_MESSAGE_BYTES`]) for reliable delivery.
    pub async fn send(&self, data: Bytes) -> Result<(), String> {
        if !self.running.load(Ordering::SeqCst) {
            return Err("serial transport not running".to_string());
        }
        if data.len() > MAX_MESSAGE_BYTES {
            return Err(format!("message too large for serial link ({} bytes)", data.len()));
        }
        let tx = self
            .outbound_tx
            .lock()
            .expect("serial lock")
            .clone()
            .ok_or_else(|| "serial writer not ready".to_string())?;
        tx.send(data)
            .await
            .map_err(|_| "serial link closed".to_string())
    }

    pub async fn shutdown(&self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return;
        }
        info!(port = %self.config.port_name, "serial transport shutting down");
        // Dropping the outbound sender ends the link task, which drops the writer channel;
        // the reader thread exits on its next 100 ms read timeout. Both port handles close.
        self.outbound_tx.lock().expect("serial lock").take();
    }
}

impl Drop for SerialTransport {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone)]
pub struct LinkConfig {
    pub port: String,
    pub rto: Duration,
    pub window: usize,
}

/// Split a message into link fragments: `[FRAG_MORE|FRAG_LAST][data…]`.
fn fragment(msg: &[u8]) -> Vec<Bytes> {
    if msg.is_empty() {
        return vec![Bytes::from_static(&[FRAG_LAST])];
    }
    let chunks: Vec<&[u8]> = msg.chunks(FRAGMENT_DATA).collect();
    let last = chunks.len() - 1;
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let mut b = BytesMut::with_capacity(c.len() + 1);
            b.extend_from_slice(&[if i == last { FRAG_LAST } else { FRAG_MORE }]);
            b.extend_from_slice(c);
            b.freeze()
        })
        .collect()
}

/// Drive one serial link until the port closes, the peer stops acknowledging, or the
/// transport is shut down. Returns the disconnect reason. Generic over the byte pipes so it
/// can be tested without hardware.
pub async fn run_link(
    cfg: LinkConfig,
    mut raw_rx: mpsc::Receiver<Vec<u8>>,
    mut outbound_rx: mpsc::Receiver<Bytes>,
    wire_tx: std::sync::mpsc::Sender<Vec<u8>>,
    event_tx: mpsc::Sender<SerialEvent>,
    running: Arc<AtomicBool>,
) -> String {
    let mut reader = SerialReader::new(cfg.rto.as_millis() as u64, cfg.window);
    let mut pending: std::collections::VecDeque<Bytes> = std::collections::VecDeque::new();
    let mut reassembly: Vec<u8> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        while let Some(frag) = pending.front() {
            match reader.prepare_send(frag.clone()) {
                Some(frame) => {
                    if wire_tx.send(frame.encode()).is_err() {
                        return "serial writer stopped".into();
                    }
                    pending.pop_front();
                }
                None => break,
            }
        }

        tokio::select! {
            raw = raw_rx.recv() => {
                let Some(raw) = raw else { return "serial port closed".into() };
                let out = reader.feed(&raw);
                for frame in out.responses.iter().chain(out.retransmits.iter()) {
                    let _ = wire_tx.send(frame.encode());
                }
                for payload in out.delivered {
                    let Some((&flag, data)) = payload.split_first() else { continue };
                    if reassembly.len() + data.len() > MAX_MESSAGE_BYTES {
                        warn!("serial message exceeds limit — dropping");
                        reassembly.clear();
                        continue;
                    }
                    reassembly.extend_from_slice(data);
                    if flag == FRAG_LAST {
                        let msg = Bytes::from(std::mem::take(&mut reassembly));
                        if event_tx.send(SerialEvent::DataReceived(msg)).await.is_err() {
                            return "engine stopped".into();
                        }
                    }
                }
            }
            msg = outbound_rx.recv(), if pending.len() < cfg.window * 4 => {
                let Some(msg) = msg else { return "graceful shutdown".into() };
                pending.extend(fragment(&msg));
            }
            _ = tick.tick() => {
                for frame in reader.poll() {
                    let _ = wire_tx.send(frame.encode());
                }
                if reader.link_failed() {
                    return "link lost — peer stopped acknowledging".into();
                }
                if !running.load(Ordering::SeqCst) {
                    return "graceful shutdown".into();
                }
            }
        }
    }
}

/// Reader task: accumulates bytes, extracts COBS frames, feeds reliability layer.
pub struct SerialReader {
    reliability: ReliabilityLayer,
    rx_buffer: BytesMut,
}

impl SerialReader {
    pub fn new(rto_ms: u64, window_size: usize) -> Self {
        Self {
            reliability: ReliabilityLayer::new()
                .with_rto(std::time::Duration::from_millis(rto_ms))
                .with_window(window_size),
            rx_buffer: BytesMut::with_capacity(8192),
        }
    }

    pub fn feed(&mut self, data: &[u8]) -> ReaderOutput {
        self.rx_buffer.extend_from_slice(data);
        // Garbage without delimiters must not grow the buffer forever.
        if self.rx_buffer.len() > 4 * super::frame::MAX_FRAME_SIZE && !self.rx_buffer.contains(&0) {
            self.rx_buffer.clear();
        }
        let mut delivered = Vec::new();
        let mut responses = Vec::new();

        while let Some(frame_data) = extract_frame(&mut self.rx_buffer) {
            match Frame::decode(&frame_data) {
                Ok(frame) => {
                    let result = self.reliability.on_receive(&frame);
                    delivered.extend(result.delivered);
                    responses.extend(result.ack_responses);
                }
                Err(e) => {
                    warn!(error = %e, "frame decode error, resyncing");
                }
            }
        }

        ReaderOutput {
            delivered,
            responses,
            retransmits: self.reliability.poll_retransmits(),
        }
    }

    pub fn prepare_send(&mut self, payload: Bytes) -> Option<Frame> {
        self.reliability.prepare_send(payload)
    }

    /// Timer-driven retransmits.
    pub fn poll(&mut self) -> Vec<Frame> {
        self.reliability.poll_retransmits()
    }

    /// A frame exhausted its retries — the link is unusable.
    pub fn link_failed(&self) -> bool {
        self.reliability.has_failed()
    }
}

pub struct ReaderOutput {
    pub delivered: Vec<Bytes>,
    pub responses: Vec<Frame>,
    pub retransmits: Vec<Frame>,
}

fn extract_frame(buf: &mut BytesMut) -> Option<Vec<u8>> {
    loop {
        let pos = buf.iter().position(|&b| b == 0x00)?;
        let frame = buf[..pos].to_vec();
        let _ = buf.split_to(pos + 1);
        // Skip empty frames (line noise / back-to-back delimiters) instead of stopping.
        if !frame.is_empty() {
            return Some(frame);
        }
    }
}

/// Serial port with human-readable label for UI dropdowns.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SerialPortEntry {
    pub path: String,
    pub label: String,
}

#[cfg(all(not(target_os = "android"), feature = "desktop"))]
fn port_label(p: &serialport::SerialPortInfo) -> String {
    use serialport::SerialPortType;
    let path = &p.port_name;
    match &p.port_type {
        SerialPortType::UsbPort(info) => {
            let product = info
                .product
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("USB Serial");
            let vendor = info
                .manufacturer
                .as_deref()
                .filter(|s| !s.is_empty());
            match vendor {
                Some(v) => format!("{v} {product} ({path})"),
                None => format!("{product} ({path})"),
            }
        }
        SerialPortType::PciPort => format!("PCI adapter ({path})"),
        SerialPortType::BluetoothPort => format!("Bluetooth ({path})"),
        SerialPortType::Unknown => path.clone(),
    }
}

/// List available serial ports with descriptive labels.
pub fn list_ports() -> Vec<SerialPortEntry> {
    #[cfg(all(not(target_os = "android"), feature = "desktop"))]
    {
        serialport::available_ports()
            .unwrap_or_default()
            .into_iter()
            .map(|p| SerialPortEntry {
                path: p.port_name.clone(),
                label: port_label(&p),
            })
            .collect()
    }
    #[cfg(not(all(not(target_os = "android"), feature = "desktop")))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_frame_from_buffer() {
        let frame = Frame::data(0, 0, Bytes::from_static(b"test"));
        let encoded = frame.encode();

        let mut buf = BytesMut::from(&encoded[..]);
        let extracted = extract_frame(&mut buf);
        assert!(extracted.is_some());
        assert!(buf.is_empty());
    }

    /// Two links joined by an in-memory cable that drops every 7th frame in each direction.
    #[tokio::test]
    async fn link_survives_frame_loss_and_reassembles() {
        fn spawn_end(
            raw_rx: mpsc::Receiver<Vec<u8>>,
        ) -> (mpsc::Sender<Bytes>, std::sync::mpsc::Receiver<Vec<u8>>, mpsc::Receiver<SerialEvent>) {
            let (out_tx, out_rx) = mpsc::channel(64);
            let (wire_tx, wire_rx) = std::sync::mpsc::channel();
            let (ev_tx, ev_rx) = mpsc::channel(64);
            let cfg = LinkConfig { port: "test".into(), rto: Duration::from_millis(150), window: 8 };
            tokio::spawn(run_link(cfg, raw_rx, out_rx, wire_tx, ev_tx, Arc::new(AtomicBool::new(true))));
            (out_tx, wire_rx, ev_rx)
        }
        fn lossy_pipe(from: std::sync::mpsc::Receiver<Vec<u8>>, to: mpsc::Sender<Vec<u8>>) {
            std::thread::spawn(move || {
                for (i, frame) in from.into_iter().enumerate() {
                    if i % 7 == 3 {
                        continue;
                    }
                    if to.blocking_send(frame).is_err() {
                        break;
                    }
                }
            });
        }
        let (a_raw_tx, a_raw_rx) = mpsc::channel(256);
        let (b_raw_tx, b_raw_rx) = mpsc::channel(256);
        let (a_out, a_wire, _a_ev) = spawn_end(a_raw_rx);
        let (_b_out, b_wire, mut b_ev) = spawn_end(b_raw_rx);
        lossy_pipe(a_wire, b_raw_tx);
        lossy_pipe(b_wire, a_raw_tx);

        let msgs: Vec<Vec<u8>> = vec![
            b"small".to_vec(),
            vec![],
            (0..30_000u32).map(|i| (i % 255) as u8).collect(),
            vec![0u8; FRAGMENT_DATA],
        ];
        for m in &msgs {
            a_out.send(Bytes::from(m.clone())).await.unwrap();
        }
        for want in &msgs {
            let got = tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    if let Some(SerialEvent::DataReceived(d)) = b_ev.recv().await {
                        return d;
                    }
                }
            })
            .await
            .expect("message not delivered");
            assert_eq!(&got[..], &want[..]);
        }
    }

    #[test]
    fn fragments_cover_message_exactly() {
        let msg = vec![9u8; FRAGMENT_DATA * 2 + 5];
        let frags = fragment(&msg);
        assert_eq!(frags.len(), 3);
        assert!(frags.iter().all(|f| f.len() <= MAX_PAYLOAD_SIZE));
        assert_eq!(frags[2][0], FRAG_LAST);
        assert_eq!(frags.iter().map(|f| f.len() - 1).sum::<usize>(), msg.len());
    }

    #[test]
    fn rto_scales_with_baud() {
        let slow = SerialConfig { baud_rate: 9600, ..Default::default() };
        let fast = SerialConfig { baud_rate: 921_600, ..Default::default() };
        assert!(slow.effective_rto() > fast.effective_rto());
        assert!(fast.effective_rto() >= Duration::from_millis(400));
    }

    #[test]
    fn reader_delivers_in_order() {
        let mut reader = SerialReader::new(200, 8);

        let f = Frame::data(0, 0, Bytes::from_static(b"hello"));
        let encoded = f.encode();
        let output = reader.feed(&encoded);

        assert_eq!(output.delivered.len(), 1);
        assert_eq!(&output.delivered[0][..], b"hello");
    }
}