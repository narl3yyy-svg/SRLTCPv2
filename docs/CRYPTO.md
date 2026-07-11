# Cryptography — SRLTCP v0.3.0

## Overview

SRLTCP implements end-to-end encryption on the wire. The hybrid handshake runs interactively between peers; application data is encrypted with **double-ratchet-2** (Signal-spec) before leaving the device. SAS codes use a canonical handshake transcript (step bodies 1→2→3) so both peers derive identical values.

Long-term **Ed25519** identity seeds are persistent across app restarts (v0.3.0).

## Primitives

| Layer | Algorithm | Notes |
|-------|-----------|-------|
| Identity | Ed25519 | QR-encoded long-term key; signs handshake frames; seed persisted |
| KEX | X25519 + ML-KEM-768 | Hybrid post-quantum (`ml-kem`; Wycheproof/ACVP-tested, not third-party audited) |
| Messaging | double-ratchet-2 0.4.0-pre.2 | Forward secrecy; AEAD via crate (aes-gcm-siv) |
| SAS | SHA-256 | 6-digit code over identities + secret + transcript |
| Compression | zstd level 3 | Folder/bulk transfer (streaming) |
| Secret hygiene | zeroize | Shared secrets / seeds zeroized on drop where applied |

## Wire Handshake

All steps are sent as `WireFrame::Handshake` (postcard) over iroh or serial:

1. **Step 1 (initiator)**: X25519 ephemeral + ML-KEM EK, signed with Ed25519 identity.
2. **Step 2 (responder)**: X25519 + ML-KEM ciphertext + bob ratchet DH pubkey (32 bytes), signed.
3. **Step 3 (initiator)**: Transcript completion marker (`0x01`), signed.

Legacy step-2 bodies that included an unused responder ML-KEM EK are still accepted when finishing KEX (prefix-only parse).

The initiator verifies each frame's Ed25519 signature and checks the identity matches the scanned QR.

## SAS

```
SAS = SHA-256(sort(pk_local, pk_remote) || shared_secret || transcript)[0:3] mod 10^6
```

Users must compare SAS out-of-band, then call `confirm_peer_trusted()` in the UI.

After trust, the **initiator** (who pasted QR / dialed) sends a `ratchet_open` system message to unlock the responder's send chain (Signal spec: responder send chain is inactive until first inbound decrypt).

## Message path

```
ChatMessage JSON → SessionRatchet.encrypt() → WireFrame::Encrypted (v3) → iroh/serial
```

## Trust states

| State | Messaging allowed |
|-------|-------------------|
| Connected | No |
| Handshaking | No |
| SasPending | No (handshake only) |
| Trusted | Yes (E2EE) |

## ML-KEM audit status

SRLTCP uses the Rust `ml-kem` crate (FIPS 203 ML-KEM-768). The crate validates against NIST ACVP and Wycheproof vectors, but **has not undergone an independent third-party audit**. Treat hybrid KEX as experimental for high-threat deployments until audit completes.

## Double Ratchet status

`double-ratchet-2` **0.4.0-pre.2** is pre-release. Integration tests and KATs cover round-trip, out-of-order decrypt, and hybrid secret symmetry. A production migration path (e.g. maintained Signal-adjacent crates or OpenMLS for groups) remains under evaluation.

## Planned: OpenMLS (multi-device / groups)

A future track may evaluate **OpenMLS** for multi-peer chat while keeping QR + SAS + Ed25519 as the trust root. No breaking wire change without an `EncryptedPayload` version bump.
