# Contributing to SRLTCP

Thank you for helping improve private peer-to-peer messaging.

## Development setup

1. Install Rust 1.85+, JDK 17 (Android), and platform deps from [docs/BUILD.md](docs/BUILD.md).
2. Core tests: `cargo test -p srltcp-core`
3. Desktop: `./run.sh --rebuild`
4. Android: `./scripts/build-android.sh`

## Guidelines

- Prefer security and correctness over clever shortcuts.
- Keep residual risks honest in `docs/SECURITY.md` when changing crypto or trust flows.
- Match existing style; keep commits focused.
- Do not add telemetry, analytics, or phone-home behavior.
- Bump `Cargo.toml` workspace version and Android `versionName`/`versionCode` together.

## Security issues

Open a GitHub issue. For critical crypto vulnerabilities, avoid posting full exploit details in public until a fix is available.
