# Build Instructions — SRLTCP v0.2.5

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

Ctrl+C or closing the window triggers graceful shutdown (releases serial ports and QUIC).

## Android — Recommended

### Full build (first time or after `git clone`)

```bash
./scripts/build-android.sh
```

This script:
1. Auto-detects JDK 17, Android SDK, and NDK
2. Cross-compiles `libsrltcp_core.so` for 3 ABIs (android feature, no serialport)
3. Generates UniFFI Kotlin bindings
4. Runs `./gradlew assembleDebug`
5. Copies APK to `dist/SRLTCPv2-0.2.5.apk`
6. Cleans Gradle caches automatically

### APK only (jniLibs already built)

```bash
./scripts/assemble-apk.sh
# equivalent to: ./scripts/build-android.sh --apk-only
```

### Manual Gradle (developers)

```bash
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk   # REQUIRED
cd android
./gradlew assembleDebug
```

### Install

```bash
adb install dist/SRLTCPv2-0.2.5.apk
```

## Cleanup

```bash
# After every APK build (automatic in build-android.sh):
./scripts/cleanup-android-build.sh

# Also from root cleanup:
./cleanup.sh --android-build

# Nuclear — remove native libs too (forces full rebuild):
./scripts/cleanup-android-build.sh --full
```

Removes: `android/app/build/`, `android/.gradle/`, `android/build/`
Keeps: `dist/*.apk`, source, `jniLibs/` (unless `--full`)

## GitHub Release (CI — recommended)

Pushing a version tag triggers `.github/workflows/release.yml`, which builds and publishes:

- `srltcp-desktop-linux-x86_64`
- `srltcp-desktop-macos-aarch64` / `srltcp-desktop-macos-x86_64`
- `srltcp-desktop-windows-x86_64.exe`
- `SRLTCPv2-<version>.apk`

```bash
# Bump version in Cargo.toml, commit, then:
git tag -a v0.2.5 -m "SRLTCP v0.2.5"
git push origin main
git push origin v0.2.5
```

Manual fallback (local artifacts in `dist/`):

```bash
./scripts/build-desktop.sh
./scripts/build-android.sh
./scripts/create-github-release.sh
```

## Rust Core

```bash
cargo check -p srltcp-core       # Fast check (desktop features)
cargo test -p srltcp-core        # Run tests
cargo build -p srltcp-core       # Library only

# Android bindgen host build (no libudev required):
cargo build -p srltcp-core --no-default-features --features android
```

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `* What went wrong: 26.0.1` | Set `JAVA_HOME` to JDK 17 |
| `Plugin compose not found` | Ensure `android/build.gradle.kts` has compose plugin |
| `ANDROID_NDK_HOME` missing | `sdkmanager "ndk;27.2.12479018"` |
| `Missing native libs` | Run full `./scripts/build-android.sh` |
| `libudev` not found (CI/Android bindgen) | Fixed in v0.2.5 via android feature gate |
| Compose BOM resolution error | BOM pinned to `2024.10.01` in `app/build.gradle.kts` |
| Port 9473 in use | `./cleanup.sh` |
| Tauri webkit error | Install `webkit2gtk-4.1-dev` |
| `run.sh` says no prebuilt | Download from Releases or use `--rebuild` |