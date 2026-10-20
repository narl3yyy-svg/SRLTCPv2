# Serial Protocol Specification

SRLTCP serial transport — COBS framing, CRC32 integrity, and sequenced reliability.

## Why This Design?

Serial links are fundamentally different from iroh/TCP streams:

| Property | Serial | iroh/TCP |
|----------|--------|----------|
| Framing | None (byte stream) | Built-in |
| Error detection | None | Checksum + retransmit |
| Bandwidth | 115200–921600 baud typical | Mbps–Gbps |
| Noise | EMI, bad cables, level shifters | Rare on Ethernet |

We need application-layer reliability without wasting precious serial bandwidth.

### Design Choices

1. **COBS (Consistent Overhead Byte Stuffing)** — Provides unambiguous frame boundaries using a `0x00` delimiter without escaping every byte. Average overhead: **~0.4%** (1 byte per 254 bytes of payload).

2. **CRC32** — Fast hardware-friendly checksum (crc32fast crate). 4-byte trailer catches burst errors and bit flips. Combined with COBS, corrupted frames are dropped and the link resynchronizes on the next valid `0x00` delimiter.

3. **Sequenced ACK/NACK** — Lightweight control packets (≤16 bytes on wire) instead of repeating full frames. Cumulative ACK piggybacked on data frames to minimize standalone control traffic.

4. **Session AEAD** — Encryption applied at the logical message level (Double Ratchet AEAD via `double-ratchet-2`), not per serial byte. The reliability layer handles frames; encryption wraps complete messages before they enter the serial stack.

## Wire Format

### Raw Frame (pre-COBS)

```
Offset  Size  Field
──────  ────  ─────
0       2     Magic: 0x53 0x52 ("SR")
2       1     Flags (see below)
3       2     Sequence number (u16 BE)
5       2     Acknowledgment (u16 BE, cumulative)
7       2     Payload length (u16 BE)
9       N     Payload (0–4096 bytes)
9+N     4     CRC32 (IEEE, big-endian)
```

### After COBS Encoding

```
[COBS-encoded bytes...] 0x00
```

The trailing `0x00` is the frame delimiter. COBS guarantees no interior `0x00` bytes.

### Flag Values

| Flag | Value | Description |
|------|-------|-------------|
| DATA | 0x01 | Application data frame |
| ACK | 0x02 | Standalone acknowledgment (no payload) |
| NACK | 0x04 | Negative ack — request retransmit of seq in payload |
| CONTROL | 0x08 | Protocol control (handshake, keepalive) |
| FIN | 0x10 | Graceful connection close |
| CHUNK | 0x20 | File transfer chunk |

## Reliability Layer

### Sequence Numbers

- 16-bit unsigned, wrapping arithmetic
- Comparison uses ring-buffer semantics: `seq_a < seq_b` iff `(b - a) mod 65536 < 32768`

### Sliding Window

- Default window: **8 frames**
- Maximum payload per frame: **4096 bytes**
- Maximum out-of-order receive buffer: **64 frames** (DoS cap)
- At 115200 baud, 8 × ~500 bytes ≈ 55ms of pipeline — good balance

### ACK Strategy

1. **Piggybacked cumulative ACK** on every outbound DATA/CHUNK frame
2. **Standalone ACK** (≤16 bytes total) when no data to send but acks pending
3. **Triple duplicate ACK** triggers fast retransmit (TCP-style)

### NACK Strategy

When an out-of-order frame arrives (e.g., seq=5 received but expecting seq=4):

```
NACK frame: flags=NACK, payload=[missing_seq: u16 BE]
```

Sender immediately retransmits the missing frame without waiting for timeout.

### Retransmission

- **Timeout (RTO):** `max(rto_ms, time to send one full window at the line rate + 200 ms)` —
  e.g. ~3 s at 115200 baud, ~35 s at 9600, so slow radios do not retransmit frames still in flight
- **Max retries:** 12 — then the link is declared lost and the peer is disconnected
  (ordered delivery cannot continue past a missing frame)
- **NACK-triggered:** Immediate retransmit, RTO timer reset

### Message Fragmentation (v0.4.0)

Every application message (an encrypted wire frame of any size up to 32 MiB) is split into
DATA-frame payloads of at most 4096 bytes:

```
fragment payload: [flag: u8][data: 0..4095 bytes]
flag 0x01 = more fragments follow, 0x00 = last fragment
```

Because the reliability layer delivers frames in order, the receiver simply concatenates
fragments until it sees `0x00`. (Before v0.4.0 the transport wrote raw, unframed bytes, so
serial links could not exchange messages.)

## Bandwidth Analysis

### ACK-Only Frame Size

```
Header:  9 bytes
Payload: 0 bytes
CRC32:   4 bytes
Total:   13 bytes raw
COBS:    ~14 bytes + delimiter = 15 bytes
```

Well under the 16-byte target.

### Data Frame Overhead (500-byte payload)

```
Raw:     9 + 500 + 4 = 513 bytes
COBS:    ~515 bytes + delimiter = 516 bytes
Overhead: 16/516 = 3.1% (including CRC + headers)
```

For maximum 4096-byte payloads:

```
Raw:     9 + 4096 + 4 = 4109 bytes
COBS:    ~4112 bytes
Overhead: 13/4109 = 0.32% framing + 4/4109 = 0.10% CRC ≈ 0.42%
```

### Effective Throughput at 115200 Baud

- Theoretical max: 11520 bytes/sec (8N1)
- With 500-byte frames + acks: ~10 KB/s application throughput
- With 4096-byte frames: ~11 KB/s (framing overhead negligible)

## File Transfer Over Serial

Files use the same encrypted file-transfer messages as iroh (offer → chunks → per-chunk
ACK), carried as ordinary fragmented messages. Over serial the sender uses 4000-byte chunks,
keeps at most 4 chunks in flight, and resends a chunk only if its ACK has not arrived within
30 s. Each ACK names just the chunk received:

```json
{ "transfer_id": "uuid", "received_chunks": [17], "missing_chunks": [] }
```

Receivers write chunks straight to a `.part` file and verify SHA-256 before moving it into
the downloads folder, so a dropped link never leaves a corrupt file behind.

## Resynchronization After Errors

1. Receiver encounters bad COBS decode or CRC mismatch
2. Frame is silently dropped
3. Receiver continues scanning for next `0x00` delimiter
4. COBS structure + magic bytes (`SR`) provide strong resync points
5. NACK informs sender of any gap created by the dropped frame
6. Link recovers within one RTO cycle (200ms typical)

## Comparison with Higher-Level Transports

On LAN/WAN (iroh), the serial reliability layer is **bypassed** — iroh provides stream framing and retransmission. The same `Envelope` and `ChatMessage` types are used; only the transport adapter differs:

```
Application Message
       │
       ├── Serial: Envelope → AES encrypt → fragments → ReliabilityLayer → COBS+CRC → UART
       └── iroh:   Envelope → AES encrypt → iroh bidirectional stream
```

## Configuration

| Parameter | Default | Range | Notes |
|-----------|---------|-------|-------|
| `baud_rate` | 115200 | 9600–921600 | Higher = faster but more error-prone on long cables |
| `rto_ms` | 400 | 50–2000 | Floor; the effective RTO also scales with baud rate |
| `window_size` | 8 | 1–32 | Larger windows improve throughput on clean links |
| `max_payload` | 4096 | — | Fixed; larger messages must be chunked |

## Implementation Reference

- Framing: `core/src/serial/frame.rs`
- Reliability: `core/src/serial/reliability.rs`
- Transport: `core/src/serial/transport.rs`