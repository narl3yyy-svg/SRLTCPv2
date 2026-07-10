# Cryptography — SRLTCP v0.2.21

## Overview

SRLTCP v0.2.13 implements end-to-end encryption on the wire. The hybrid handshake runs interactively between peers; application data is encrypted with **double-ratchet-2** (Signal-spec) before leaving the device. SAS codes use a canonical handshake transcript (step bodies 1→2→3) so both peers derive identical values.

## Primitives

| Layer | Algorithm | Notes |
|-------|-----------|-------|
| Identity | Ed25519 | QR-encoded long-term key; signs handshake frames |
| KEX | X25519 + ML-KEM-768 | Hybrid post-quantum (ml-kem crate; Wycheproof-tested, not independently audited) |
| Messaging | double-ratchet-2 (Signal) | Forward secrecy; ChaCha20-Poly1305 via crate |
| SAS | SHA-256 | 6-digit code over identities + secret + transcript |
| Compression | zstd level 3 | Folder/bulk transfer (streaming) |

## Wire Handshake (v0.2.10)

All steps are sent as `WireFrame::Handshake` (postcard) over iroh or serial:

1. **Step 1 (initiator)**: X25519 ephemeral + ML-KEM EK, signed with Ed25519 identity.
2. **Step 2 (responder)**: Hybrid KEX response + bob ratchet DH pubkey (32 bytes), signed.
3. **Step 3 (initiator)**: Transcript completion marker, signed.

The responder verifies each frame's Ed25519 signature and checks the identity matches the scanned QR.

## SAS

```
SAS = SHA-256(sort(pk_local, pk_remote) || shared_secret || transcript)[0:3] mod 10^6
```

Users must compare SAS out-of-band, then call `confirm_peer_trusted()` in the UI.

After trust, the **initiator** (who pasted QR / dialed) sends a `ratchet_open` system message to unlock the responder's send chain (Signal spec: responder `cks` is `None` until first inbound decrypt).

## Message path

```
ChatMessage JSON → SessionRatchet.encrypt() → WireFrame::Encrypted (v3) → iroh/serial
```

Inbound path reverses decryption before parsing `ChatMessage`.

## Trust states

| State | Messaging allowed |
|-------|-------------------|
| Connected | No |
| Handshaking | No |
| SasPending | No (handshake only) |
| Trusted | Yes (E2EE) |

## ML-KEM audit status

SRLTCP uses the Rust `ml-kem` crate (FIPS 203 ML-KEM-768). The crate validates against NIST ACVP and Wycheproof vectors, but **has not undergone an independent third-party audit**. Treat hybrid KEX as experimental for high-threat deployments until audit completes.

## Planned: OpenMLS migration (multi-device / groups)

1:1 messaging today uses **double-ratchet-2** (Signal-spec). A future **v0.3.x** track will evaluate **OpenMLS** for:

- Audited MLS group semantics (when multi-peer chat is needed)
- Cleaner ratchet state persistence across app restarts
- Optional integration with hardware keystores

Migration plan (draft):

1. Keep QR + SAS + Ed25519 identity as the trust root.
2. Run OpenMLS epoch setup inside the existing signed handshake transcript.
3. Dual-stack period: double-ratchet-2 for v0.2 peers, MLS for v0.3+.
4. No breaking wire change without a `EncryptedPayload` version bump.