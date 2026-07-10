# Security — SRLTCP v0.2.21

## Goals

1. **E2EE** — Peers encrypt all chat/file signaling with Signal-spec double-ratchet-2 after trust confirmation.
2. **Forward secrecy** — Ratchet chains limit key compromise blast radius.
3. **Post-quantum hybrid KEX** — ML-KEM-768 + X25519 against harvest-now-decrypt-later.
4. **Authenticated handshake** — Ed25519-signed frames bound to QR identity.
5. **MITM detection** — SAS includes handshake transcript; user must verify before trust.

## Threat model

| Threat | Mitigation (v0.2.13) |
|--------|---------------------|
| Passive network tap | Encrypted payloads after trust |
| Active MITM on first contact | SAS + signed handshake tied to QR keys |
| Impersonation | Ed25519 signature on every handshake step |
| Replay (app layer) | Ratchet message counters |
| Quantum harvest | ML-KEM-768 hybrid |

## Trust establishment

1. Exchange QR v4 codes (Ed25519 identity + iroh ticket).
2. Run wire handshake (automatic on connect).
3. Compare 6-digit SAS verbally or in person.
4. Tap **Codes Match** → `confirm_peer_trusted()` unlocks messaging.

**Without step 4, messages are rejected** with "peer not trusted".

## Out of scope

- Compromised endpoint (malware on device)
- User skipping SAS verification
- DDoS on iroh relay path
- WebRTC media uses STUN/DTLS-SRTP (signaling is E2EE; media not double-ratchet wrapped)
- `ml-kem` crate: Wycheproof-tested, **not independently audited**
- `double-ratchet-2` pre-release crate — evaluate before high-threat deployment

## Reporting

Open a GitHub issue on [SRLTCPv2](https://github.com/narl3yyy-svg/SRLTCPv2) for security concerns.