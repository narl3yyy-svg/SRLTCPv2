# Security — SRLTCP v0.2.30

## Goals

1. **E2EE** — Chat and file signaling use Signal-spec `double-ratchet-2` after SAS trust confirmation.
2. **Forward secrecy** — Ratchet chains limit key-compromise blast radius.
3. **Post-quantum hybrid KEX** — ML-KEM-768 + X25519 against harvest-now-decrypt-later.
4. **Authenticated handshake** — Ed25519-signed frames bound to QR identity.
5. **MITM detection** — SAS includes handshake transcript; users must verify before trust.

## Threat model

| Threat | Mitigation |
|--------|------------|
| Passive network tap | Encrypted payloads after trust |
| Active MITM on first contact | SAS + signed handshake tied to QR keys |
| Impersonation | Ed25519 signature on every handshake step |
| Replay (app layer) | Ratchet message counters |
| Quantum harvest | ML-KEM-768 hybrid |

**Out of scope:** compromised endpoint, user skipping SAS, DDoS on iroh relays.

## Trust establishment

1. Exchange QR v4 (Ed25519 identity + iroh ticket).
2. Wire handshake runs automatically on connect.
3. Compare 6-digit SAS in person or over a trusted channel.
4. Confirm **Codes Match** → `confirm_peer_trusted()` unlocks messaging.

Saved verified contacts reconnect without re-SAS when the stored pubkey matches (auto-trusted path).

## Residual risks (honest audit)

| Risk | Detail |
|------|--------|
| **Unaudited crypto** | `ml-kem` 0.3 is Wycheproof-tested but not independently audited. `double-ratchet-2` 0.4.0-pre.2 is **pre-release** — a bug is catastrophic. |
| **Key storage** | Identity and trusted pubkeys live in plain SharedPreferences (Android) / app localStorage (desktop). No OS keystore, sealed storage, or hardware binding. |
| **WebRTC media** | Signaling (SDP/ICE) is E2EE over the ratchet. Media uses standard STUN + DTLS-SRTP — **not** Double-Ratchet wrapped. |
| **iroh relays** | N0 public relays see connection metadata (who, when, sizes). Content is E2EE. |
| **No epoch rotation** | Long-lived sessions rely on the ratchet alone; no separate continuous rekey beyond ratchet steps. |
| **Chat persistence** | Per-peer chat history is stored locally (JSON). Not encrypted at rest beyond OS filesystem permissions. |
| **Serial** | Desktop-only; Android has no serial transport. |
| **Engine maturity** | Large async Rust engine (~80k LOC surface): aliases, 500-msg queues, transfers, calls, auto-reconnect. Many race fixes shipped; surface still young. |
| **No formal verification** | No third-party audit, no constant-time audit of hybrid combine, no public CodeQL/Dependabot program. |
| **Trust store** | Local, mutable by the app process. Device compromise = impersonation until re-verified. |

## Verdict

Design is modern and layered for stated goals. Implementation quality is high for a project of this age. **Still experimental.** Suitable for personal / trusted-circle / air-gapped serial use. **Not** ready for high-threat targets without independent audit of both crypto crates, engine review, and key-storage hardening.

## Reporting

Open a GitHub issue on [SRLTCPv2](https://github.com/narl3yyy-svg/SRLTCPv2) for security concerns.