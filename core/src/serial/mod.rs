//! Highly reliable serial transport layer.
//!
//! COBS framing + CRC32 integrity + sequenced ACK/NACK reliability.
//! See `docs/SERIAL_PROTOCOL.md` for full specification.

pub mod frame;
pub mod reliability;
pub mod transport;

pub use frame::{Frame, FrameFlags, FrameError, MAX_PAYLOAD_SIZE};
pub use reliability::{ReliabilityLayer, ReceiveResult, DEFAULT_RTO_MS, DEFAULT_WINDOW_SIZE};
pub use transport::{SerialConfig, SerialEvent, SerialTransport, SerialReader, list_ports};