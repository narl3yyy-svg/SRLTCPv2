# Build Instructions — SRLTCP v0.3.1

## Prerequisites

| Component | Requirement |
|-----------|-------------|
| Rust | 1.85+ (only needed for `--rebuild` or developer builds) |
| Desktop Linux | `webkit2gtk-4.1`, `gtk3`, `libudev-dev`, `base-devel` |
| Android SDK | API 35 (`sdkmanager "platforms;android-35"`) |
| Android NDK | r27+ (`sdkmanager "ndk;27.2.12479018"`) |
| JDK | **17 only** — JDK 21+ breaks Gradle |
| cargo-ndk | `cargo install cargo-ndk` |

## Desktop (End Users)

```bash
./run.sh          # Linux/macOS — downloads prebuilt, no compile
run.bat           # Windows
```

Launchers use prebuilt binaries from GitHub Releases. Compile only when needed:

```bash
./run.sh --rebuild                              # Build + launch
cargo build --release -p srltcp-desktop         # Manual rebuild
./scripts/build-desktop.sh                      # Stage prebuilt in dist/
```

Release builds use LTO + strip (`Cargo.toml` `[profile.release]`).

Ctrl+C or closing the window triggers graceful shutdown.

### Identity location (desktop)

| OS | Path |
|----|------|
| Linux | `~/.local/share/srltcp/identity.seed` |
| macOS | `~/Library/Application Support/srltcp/identity.seed` |
| Windows | `%APPDATA%\srltcp\identity.seed` |

Override with `SRLTCP_DATA_DIR`.

## Android — Recommended

### Full build (slim arm64 APK — default)

```bash
./scripts/build-android.sh
```

Produces `dist/SRLTCPv2-0.3.1.apk` (release, minified, arm64-v8a only).

### Universal multi-ABI APK

```bash
SRLTCP_UNIVERSAL_APK=1 ./scripts/build-android.sh
```

### APK only (jniLibs already built)

```bash
./scripts/assemble-apk.sh
```

### Install

```bash
adb uninstall com.srltcp.v2 2>/dev/null || true
adb install dist/SRLTCPv2-0.3.1.apk
```

## Cleanup

```bash
./scripts/cleanup-android-build.sh
./cleanup.sh --android-build
./scripts/cleanup-android-build.sh --full   # also removes jniLibs
```

## GitHub Release (CI)

Pushing a version tag triggers `.github/workflows/release.yml`:

- `srltcp-desktop-linux-x86_64`
- `srltcp-desktop-macos-aarch64` / `srltcp-desktop-macos-x86_64`
- `srltcp-desktop-windows-x86_64.exe`
- `SRLTCPv2-<version>.apk`

```bash
git tag -a v0.3.1 -m "SRLTCP v0.3.1"
git push origin main
git push origin v0.3.1
```

## Rust Core

```bash
cargo check -p srltcp-core
cargo test -p srltcp-core
cargo build -p srltcp-core --no-default-features --features android
```

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `* What went wrong: 26.0.1` | Set `JAVA_HOME` to JDK 17 |
| `ANDROID_NDK_HOME` missing | `sdkmanager "ndk;27.2.12479018"` |
| `Missing native libs` | Run full `./scripts/build-android.sh` |
| Tauri webkit error | Install `webkit2gtk-4.1-dev` |
| `run.sh` says no prebuilt | Download from Releases or use `--rebuild` |
| Identity changed after reinstall | Seed wiped with app data — re-verify SAS with peers |
