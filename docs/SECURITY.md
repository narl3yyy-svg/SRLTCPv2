# Security

SRLTCP v0.2.0 security model, threat analysis, and security properties.

## Security Goals

1. **End-to-end encryption** — No third party (including relays) can read message content
2. **Forward secrecy** — Past messages remain secure if current keys are compromised
3. **Post-compromise security** — Future messages heal after key compromise (Double Ratchet)
4. **Post-quantum resistance** — Hybrid ML-KEM-768 protects against harvest-now-decrypt-later
5. **Authentication** — Ed25519 identity + SAS verification prevents impersonation
6. **Integrity** — AEAD encryption + CRC32 (serial) / QUIC checksums (network) detect tampering

## Threat Model

### In Scope

| Threat | Mitigation |
|--------|------------|
| Network eavesdropper (LAN/WAN) | QUIC + E2EE (AES-256-GCM) |
| Serial line tapping | Session AEAD encryption |
| Active MITM on first contact | SAS out-of-band verification |
| Relay operator reading traffic | E2EE — relay sees only ciphertext |
| Message tampering | GCM authentication tags |
| Replay attacks | Monotonic nonces + sequence numbers |
| Quantum computer (future) | ML-KEM-768 hybrid KEX |
| Stolen device (current session) | Forward secrecy limits exposure |
| Stolen device (past sessions) | Key erasure on shutdown |

### Out of Scope

| Threat | Notes |
|--------|-------|
| Compromised endpoint (malware) | Cannot protect against device-level compromise |
| Physical device seizure with storage | Local key storage needs OS-level protection (future: secure enclave) |
| Social engineering of SAS | Users must actually verify the 6-digit code |
| RF jamming on serial | Physical layer — detected as connection loss |
| DDoS on QUIC port | No rate limiting in v0.2.0 |

## Trust Establishment

```
┌─────────┐                    ┌─────────┐
│  Alice  │                    │   Bob   │
└────┬────┘                    └────┬────┘
     │  1. Display QR (Ed25519 pk)  │
     │ ─────────────────────────────>│  2. Scan QR
     │                               │
     │  3. Hybrid X25519+ML-KEM KEX  │
     │ <────────────────────────────>│
     │                               │
     │  4. SAS: "482910"             │
     │ <──── verbal confirm ────────>│  4. SAS: "482910"
     │                               │
     │  5. Trusted session ✓         │
     │ <════ Double Ratchet E2EE ═══>│
```

**Critical:** SAS verification is the user's responsibility. Without it, a MITM can substitute keys during the initial handshake.

## Key Storage

| Key | Storage | Lifetime |
|-----|---------|----------|
| Ed25519 identity | Generated in memory; persisted to app data (future) | Permanent |
| ML-KEM ephemeral | Memory only | Per handshake |
| X25519 ephemeral | Memory only | Per handshake / ratchet step |
| Session keys | Memory only | Per session |
| Ratchet state | Memory only | Per conversation |

On graceful shutdown, all ephemeral and session keys are dropped. The Ed25519 identity key is the only long-term secret.

## Serial-Specific Security

- **No encryption at byte level** — AEAD operates on complete messages, not per-byte, to minimize overhead
- **ACK/NACK frames are unencrypted** — They contain only sequence numbers, no payload data
- **CRC32 is not cryptographic** — It detects accidental corruption only; GCM auth tags detect malicious tampering at the message layer
- **Resync after errors** — Corrupted frames are dropped; the link recovers without exposing plaintext

## Android Background Security

The Foreground Service:

- Displays a persistent notification (user awareness)
- Holds P2P sessions in memory with the same E2EE as desktop
- Does **not** store messages on disk by default
- Stops only on Force Stop (user intent) or `shutdown()` call
- Uses `android:stopWithTask="false"` to survive UI dismissal

## Graceful Shutdown Security

On Ctrl+C or app close:

1. FIN frames sent on serial links
2. QUIC connections closed with `0x00` error code
3. All session keys zeroed (via Drop)
4. Serial ports released
5. Network sockets closed
6. No key material written to temp files

## Reporting Vulnerabilities

Please report security issues responsibly. Do not open public GitHub issues for vulnerabilities.

## Compliance Notes

- ML-KEM-768: NIST FIPS 203
- AES-256-GCM: NIST SP 800-38D
- Ed25519: RFC 8032
- X25519: RFC 7748
- HKDF: RFC 5869

Cryptographic implementations use audited crates (aws-lc-rs, ed25519-dalek, ml-kem) but the **composition and protocol logic has not been independently audited**. Use at your own risk for production deployments with sensitive data.