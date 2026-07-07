# Build Instructions — SRLTCP v0.2.1

## Prerequisites

| Component | Requirement |
|-----------|-------------|
| Rust | 1.85+ (auto-installed by `run.sh`) |
| Desktop Linux | `webkit2gtk-4.1`, `gtk3`, `base-devel` |
| Android SDK | API 35 (`sdkmanager "platforms;android-35"`) |
| Android NDK | r27+ (`sdkmanager "ndk;27.2.12479018"`) |
| JDK | **17 only** — JDK 21+ breaks Gradle |
| cargo-ndk | `cargo install cargo-ndk` |

## Desktop

```bash
./run.sh                              # Build (first run) + launch
cargo build --release -p srltcp-desktop   # Manual rebuild
```

Ctrl+C triggers graceful shutdown (releases serial ports and QUIC).

## Android — Recommended

### Full build (first time or after `git clone`)

```bash
./scripts/build-android.sh
```

This script:
1. Auto-detects JDK 17, Android SDK, and NDK
2. Cross-compiles `libsrltcp_core.so` for 3 ABIs
3. Generates UniFFI Kotlin bindings
4. Runs `./gradlew assembleDebug`
5. Copies APK to `dist/SRLTCPv2-v0.2.1-debug.apk`
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
adb install dist/SRLTCPv2-v0.2.1-debug.apk
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

## GitHub Release

```bash
./scripts/build-android.sh
git add -A && git commit -m "Release v0.2.2"
./scripts/create-github-release.sh    # edit VERSION in script first
```

Or manually:

```bash
git tag -a v0.2.1 -m "SRLTCP v0.2.1"
git push origin main --tags
gh release create v0.2.1 dist/SRLTCPv2-v0.2.1-debug.apk
```

## Rust Core

```bash
cargo check -p srltcp-core       # Fast check
cargo test -p srltcp-core        # Run tests
cargo build -p srltcp-core       # Library only
```

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `* What went wrong: 26.0.1` | Set `JAVA_HOME` to JDK 17 |
| `Plugin compose not found` | Ensure `android/build.gradle.kts` has compose plugin |
| `ANDROID_NDK_HOME` missing | `sdkmanager "ndk;27.2.12479018"` |
| `Missing native libs` | Run full `./scripts/build-android.sh` |
| Compose BOM resolution error | BOM pinned to `2024.10.01` in `app/build.gradle.kts` |
| Port 9473 in use | `./cleanup.sh` |
| Tauri webkit error | Install `webkit2gtk-4.1-dev` |