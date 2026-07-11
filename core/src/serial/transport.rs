//! Async serial port transport with COBS framing and reliability layer.

use std::sync::Arc;

#[cfg(all(not(target_os = "android"), feature = "desktop"))]
use std::io::{Read, Write};

use bytes::{Bytes, BytesMut};
use tokio::sync::{mpsc, RwLock};
#[cfg(all(not(target_os = "android"), feature = "desktop"))]
use tokio::sync::Mutex;
use tracing::{info, warn};
#[cfg(all(not(target_os = "android"), feature = "desktop"))]
use tracing::debug;

#[cfg(all(not(target_os = "android"), feature = "desktop"))]
use std::time::Duration;

#[cfg(all(not(target_os = "android"), feature = "desktop"))]
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
    write_tx: Arc<tokio::sync::Mutex<Option<mpsc::Sender<Bytes>>>>,
    #[cfg(all(not(target_os = "android"), feature = "desktop"))]
    port: Arc<Mutex<Option<Box<dyn SerialPort>>>>,
}

impl SerialTransport {
    pub fn new(config: SerialConfig) -> (Self, mpsc::Receiver<SerialEvent>) {
        let (event_tx, event_rx) = mpsc::channel(256);

        let transport = Self {
            config,
            running: Arc::new(RwLock::new(false)),
            event_tx,
            write_tx: Arc::new(tokio::sync::Mutex::new(None)),
            #[cfg(all(not(target_os = "android"), feature = "desktop"))]
            port: Arc::new(Mutex::new(None)),
        };

        (transport, event_rx)
    }

    pub async fn start(&self) -> Result<(), String> {
        #[cfg(not(all(not(target_os = "android"), feature = "desktop")))]
        {
            return Err("serial transport is not available on this platform".to_string());
        }

        #[cfg(all(not(target_os = "android"), feature = "desktop"))]
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

            let port_arc = Arc::new(Mutex::new(port));
            {
                let mut guard = self.port.lock().await;
                *guard = None;
            }

            let (write_tx, mut write_rx) = mpsc::channel::<Bytes>(256);
            *self.write_tx.lock().await = Some(write_tx);

            let running_w = self.running.clone();
            let port_write = port_arc.clone();
            tokio::spawn(async move {
                while *running_w.read().await {
                    match write_rx.recv().await {
                        Some(data) => {
                            let mut guard = port_write.lock().await;
                            if let Err(e) = guard.write_all(&data) {
                                warn!(error = %e, "serial write failed");
                                break;
                            }
                            let _ = guard.flush();
                        }
                        None => break,
                    }
                }
            });

            let running_r = self.running.clone();
            let event_tx = self.event_tx.clone();
            let port_name_r = self.config.port_name.clone();
            let port_read = port_arc.clone();
            let rto = self.config.rto_ms;
            let window = self.config.window_size;
            tokio::task::spawn_blocking(move || {
                let mut reader = SerialReader::new(rto, window);
                let mut buf = [0u8; 4096];
                while *running_r.blocking_read() {
                    let n = {
                        let mut guard = port_read.blocking_lock();
                        match guard.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                            Err(e) => {
                                let _ = event_tx.blocking_send(SerialEvent::Error(format!(
                                    "serial read error: {e}"
                                )));
                                break;
                            }
                        }
                    };
                    let output = reader.feed(&buf[..n]);
                    for payload in output.delivered {
                        let _ = event_tx.blocking_send(SerialEvent::DataReceived(payload));
                    }
                    for frame in output
                        .responses
                        .iter()
                        .chain(output.retransmits.iter())
                    {
                        let encoded = frame.encode();
                        let mut guard = port_read.blocking_lock();
                        let _ = guard.write_all(&encoded);
                        let _ = guard.flush();
                    }
                }
                let _ = event_tx.blocking_send(SerialEvent::Disconnected {
                    port: port_name_r,
                    reason: "connection closed".to_string(),
                });
            });

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
        let tx = self.write_tx.lock().await;
        let Some(ref write_tx) = *tx else {
            return Err("serial writer not ready".to_string());
        };
        write_tx
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

        #[cfg(all(not(target_os = "android"), feature = "desktop"))]
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