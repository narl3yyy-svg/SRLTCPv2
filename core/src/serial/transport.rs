//! Async serial port transport with COBS framing and reliability layer.

use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, info, warn};

#[cfg(not(target_os = "android"))]
use std::time::Duration;

#[cfg(not(target_os = "android"))]
use serialport::SerialPort;

use super::frame::Frame;
use super::reliability::ReliabilityLayer;

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
            rto_ms: 200,
            window_size: 8,
        }
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
    running: Arc<RwLock<bool>>,
    event_tx: mpsc::Sender<SerialEvent>,
    write_tx: mpsc::Sender<Bytes>,
    #[cfg(not(target_os = "android"))]
    port: Arc<Mutex<Option<Box<dyn SerialPort>>>>,
}

impl SerialTransport {
    pub fn new(config: SerialConfig) -> (Self, mpsc::Receiver<SerialEvent>) {
        let (event_tx, event_rx) = mpsc::channel(256);
        let (write_tx, _write_rx) = mpsc::channel::<Bytes>(256);

        let transport = Self {
            config,
            running: Arc::new(RwLock::new(false)),
            event_tx,
            write_tx,
            #[cfg(not(target_os = "android"))]
            port: Arc::new(Mutex::new(None)),
        };

        (transport, event_rx)
    }

    pub async fn start(&self) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            return Err("serial transport is not available on Android".to_string());
        }

        #[cfg(not(target_os = "android"))]
        {
            let mut running = self.running.write().await;
            if *running {
                return Ok(());
            }

            let port = serialport::new(&self.config.port_name, self.config.baud_rate)
                .timeout(Duration::from_millis(100))
                .open()
                .map_err(|e| format!("failed to open {}: {e}", self.config.port_name))?;

            info!(port = %self.config.port_name, baud = self.config.baud_rate, "serial port opened");

            {
                let mut guard = self.port.lock().await;
                *guard = Some(port);
            }

            *running = true;
            drop(running);

            let _ = self
                .event_tx
                .send(SerialEvent::Connected {
                    port: self.config.port_name.clone(),
                })
                .await;

            Ok(())
        }
    }

    pub async fn send(&self, data: Bytes) -> Result<(), String> {
        if !*self.running.read().await {
            return Err("transport not running".to_string());
        }
        self.write_tx
            .send(data)
            .await
            .map_err(|e| format!("write channel closed: {e}"))
    }

    pub async fn shutdown(&self) {
        let mut running = self.running.write().await;
        if !*running {
            return;
        }
        *running = false;

        info!(port = %self.config.port_name, "serial transport shutting down");

        #[cfg(not(target_os = "android"))]
        {
            let mut guard = self.port.lock().await;
            if let Some(port) = guard.take() {
                drop(port);
                debug!("serial port closed");
            }
        }

        let _ = self
            .event_tx
            .send(SerialEvent::Disconnected {
                port: self.config.port_name.clone(),
                reason: "graceful shutdown".to_string(),
            })
            .await;
    }

    pub fn is_running(&self) -> Arc<RwLock<bool>> {
        self.running.clone()
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
}

pub struct ReaderOutput {
    pub delivered: Vec<Bytes>,
    pub responses: Vec<Frame>,
    pub retransmits: Vec<Frame>,
}

fn extract_frame(buf: &mut BytesMut) -> Option<Vec<u8>> {
    if let Some(pos) = buf.iter().position(|&b| b == 0x00) {
        let frame = buf[..pos].to_vec();
        let _ = buf.split_to(pos + 1);
        if frame.is_empty() {
            return None;
        }
        Some(frame)
    } else {
        None
    }
}

/// List available serial ports.
pub fn list_ports() -> Vec<String> {
    #[cfg(target_os = "android")]
    {
        Vec::new()
    }
    #[cfg(not(target_os = "android"))]
    {
        serialport::available_ports()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.port_name)
            .collect()
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