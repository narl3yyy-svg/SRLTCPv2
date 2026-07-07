# Cryptography

SRLTCP v0.2.0 cryptographic design — bleeding-edge security with bandwidth efficiency.

## Primitive Summary

| Purpose | Algorithm | Crate | Notes |
|---------|-----------|-------|-------|
| Long-term identity | Ed25519 | ed25519-dalek | 32-byte public keys |
| Classical KEX | X25519 | x25519-dalek | Ephemeral DH |
| Post-quantum KEX | ML-KEM-768 | ml-kem | FIPS 203 |
| Handshake | Noise XX + hybrid | snow + custom | PQ-augmented |
| Messaging | Double Ratchet | custom | AES-256-GCM chains |
| Symmetric | AES-256-GCM | aes-gcm (aws-lc-rs) | Hardware-accelerated |
| KDF | HKDF-SHA256 | hkdf | Key derivation |
| SAS | SHA-256 | sha2 | 6-digit verification code |
| QR encoding | Base64 URL-safe | base64 | Identity distribution |

## Identity (Ed25519)

Each device generates a long-term Ed25519 keypair on first launch:

```
SigningKey ← random 32 bytes
VerifyingKey ← derived public key
```

The public key is encoded for QR distribution:

```
QR payload = base64url(version_byte || public_key_32_bytes)
           = base64url(0x02 || ed25519_pk)
```

Peers scan QR codes to obtain each other's long-term identity before initiating encrypted contact.

## Hybrid Key Exchange (X25519 + ML-KEM-768)

Classical X25519 alone is vulnerable to harvest-now-decrypt-later attacks by future quantum computers. SRLTCP combines it with ML-KEM-768 (NIST FIPS 203):

### Initiator → Responder (Message 1)

```
[x25519_ephemeral_pk: 32 bytes]
[mlkem_encapsulation_key: 1184 bytes]
```

### Responder → Initiator (Message 2)

```
[x25519_ephemeral_pk: 32 bytes]
[mlkem_ciphertext: 1088 bytes]      ← encapsulates shared secret to initiator's ML-KEM key
[mlkem_encapsulation_key: 1184 bytes] ← responder's ML-KEM key for reverse encapsulation
```

### Shared Secret Derivation

```
x25519_ss = X25519(eph_secret, remote_eph_pk)
mlkem_ss = ML-KEM-768 decapsulate/initiator + encapsulate/responder
combined = HKDF-SHA256(
    ikm = x25519_ss || mlkem_ss,
    info = "srltcp-v2-hybrid-kex"
)
→ 32-byte shared secret
```

Both classical and post-quantum components must be compromised to break the session.

## Short Authentication String (SAS)

After key exchange, both peers compute:

```
SAS = SHA-256(sorted(local_pk, remote_pk) || shared_secret) mod 10^6
```

Displayed as a 6-digit code (e.g., `482910`). Users verbally compare codes to detect MITM attacks. Canonical key ordering prevents reflection attacks.

## Session Encryption (AES-256-GCM)

Derived from the hybrid shared secret:

```
session_key = HKDF-SHA256(
    salt = handshake_transcript_hash,
    ikm = shared_secret,
    info = "srltcp-v2-session"
)
```

Messages encrypted with monotonically incrementing 96-bit nonces. GCM provides authenticated encryption — tampered ciphertext is rejected.

Hardware acceleration via `aws-lc-rs` (OpenSSL-derived, AES-NI on x86, ARMv8 crypto extensions on mobile).

## Double Ratchet (Messaging)

After the initial handshake, ongoing messages use a Double Ratchet for:

- **Forward secrecy:** Compromised keys don't reveal past messages
- **Post-compromise security:** New DH ratchet steps heal after key compromise

### State

```
root_key          — updated on each DH ratchet step
send_chain_key    — symmetric chain for outbound messages
recv_chain_key    — symmetric chain for inbound messages
dh_send_keypair   — current ephemeral DH keypair
dh_recv_public    — remote's current DH public key
```

### Per-Message

1. Encrypt with current send chain key (AES-256-GCM)
2. Increment message counter
3. Every 256 messages: advance chain key via HKDF
4. On receiving new remote DH public key: perform DH ratchet step

### Bandwidth Efficiency on Serial

The ratchet operates on **logical messages** (complete chat messages, file chunks), not individual serial frames. A 100-byte text message is:

1. Ratchet-encrypted → ~116 bytes (16-byte GCM tag)
2. Wrapped in Envelope JSON → ~150 bytes
3. Sent through reliability layer → 1 serial frame
4. COBS + CRC overhead → ~155 bytes on wire

Tiny ACK frames (15 bytes) are never encrypted — they carry no sensitive data.

## Noise Protocol Integration

The handshake follows Noise XX pattern augmented with ML-KEM:

```
-> e, s, mlkem_ek
<- e, ee, s, es, mlkem_ct, mlkem_ek
-> se, mlkem_ct2
```

Identity keys (Ed25519) are transmitted encrypted within the Noise handshake, protected by the ephemeral DH keys.

## QR Code Discovery

1. Alice displays QR containing her `qr_payload`
2. Bob scans QR, obtains Alice's Ed25519 public key
3. Bob initiates hybrid handshake
4. Both display SAS for verbal verification
5. On match, session is trusted

## WebRTC E2EE

Voice/video calls use WebRTC for media transport (DTLS-SRTP). Signaling (SDP offer/answer, ICE candidates) is exchanged over the established Double Ratchet encrypted channel. An additional application-layer key is derived for SRTP via:

```
webrtc_key = HKDF-SHA256(ratchet_root, info = "srltcp-v2-webrtc")
```

## Implementation Files

| Component | File |
|-----------|------|
| Identity | `core/src/crypto/identity.rs` |
| Hybrid KEX | `core/src/crypto/handshake.rs` |
| Double Ratchet | `core/src/crypto/ratchet.rs` |