# Security — SRLTCP v0.3.1

## Goals

1. **E2EE** — Chat and file signaling use Signal-spec `double-ratchet-2` after SAS trust confirmation.
2. **Forward secrecy** — Ratchet chains limit key-compromise blast radius.
3. **Post-quantum hybrid KEX** — ML-KEM-768 + X25519 against harvest-now-decrypt-later.
4. **Authenticated handshake** — Ed25519-signed frames bound to QR identity.
5. **MITM detection** — SAS includes handshake transcript; users must verify before trust.
6. **Stable identity** — Long-term Ed25519 seed persists across restarts (v0.3.1).

## Threat model

| Threat | Mitigation |
|--------|------------|
| Passive network tap | Encrypted payloads after trust |
| Active MITM on first contact | SAS + signed handshake tied to QR keys |
| Impersonation | Ed25519 signature on every handshake step |
| Replay (app layer) | Ratchet message counters |
| Quantum harvest | ML-KEM-768 hybrid |
| Identity loss on restart | Persisted seed (desktop file 0600; Android EncryptedSharedPreferences) |
| QR ticket swap / identity spoof via refresh | Refresh accepted only if Ed25519 matches session peer |

**Out of scope:** compromised endpoint, user skipping SAS, DDoS on iroh relays, physical device seizure without full-disk encryption.

## Trust establishment

1. Exchange QR v4 (Ed25519 identity + iroh ticket).
2. Wire handshake runs automatically on connect.
3. Compare 6-digit SAS in person or over a trusted channel.
4. Confirm **Codes Match** → `confirm_peer_trusted()` unlocks messaging.

Saved verified contacts reconnect without re-SAS when the stored pubkey matches (auto-trusted path). A **new** identity for a previously known person always requires a fresh SAS compare.

## Residual risks (honest audit)

| Risk | Detail |
|------|--------|
| **Unaudited crypto** | `ml-kem` 0.3 is Wycheproof-tested but not independently audited. `double-ratchet-2` **0.4.0-pre.2** is pre-release — a bug is catastrophic. |
| **Key storage** | Desktop: seed file under app data (`~/.local/share/srltcp/identity.seed`, mode 0600). Android: EncryptedSharedPreferences (AES-GCM via Android Keystore master key) when available. Not hardware-bound on all devices. |
| **WebRTC media** | Signaling (SDP/ICE) is E2EE over the ratchet. Media uses standard STUN + DTLS-SRTP — **not** Double-Ratchet wrapped. UI discloses this. |
| **iroh relays** | N0 public relays see connection metadata (who, when, sizes). Content is E2EE. |
| **Chat persistence** | Per-peer chat history is stored locally (JSON / SharedPreferences). Not encrypted at rest beyond OS sandbox. |
| **Serial** | Desktop-only; recv buffer capped (DoS resistance). Android has no serial transport. |
| **No formal verification** | No third-party audit, no constant-time audit of hybrid combine. |
| **Trust store** | Local, mutable by the app process. Device compromise = impersonation until re-verified. |
| **Legacy JSON wire** | Still accepted for frame deserialize (migration); prefer postcard `SR` frames. |

## v0.3.1 hardening summary

- Persistent identity (desktop + Android)
- `Zeroizing` / `Zeroize` on hybrid shared secrets and identity seeds
- Hybrid step-2 body simplified (removed unused ML-KEM EK bytes); legacy responses still parse
- QR refresh must match canonical peer Ed25519
- Serial out-of-order buffer hard cap
- Honest WebRTC module documentation
- Release profile: LTO, strip, size-oriented opt-level

## Verdict

Design is modern and layered for stated goals. v0.3.1 is a material step toward daily-driver usability (stable identity, size, honesty). **Still experimental** for high-threat targets until independent audit of crypto crates and engine review.

## Reporting

Open a GitHub issue on [SRLTCPv2](https://github.com/narl3yyy-svg/SRLTCPv2) for security concerns. Prefer private disclosure for critical crypto bugs when possible.
