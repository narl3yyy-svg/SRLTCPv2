# Build Instructions

Building SRLTCP v0.2.1 on all platforms.

## Prerequisites

### All Platforms

- **Rust** 1.85+ (installed automatically by `run.sh` / `run.bat`)
- **Git**

### Desktop (Tauri v2)

| Platform | Additional Dependencies |
|----------|------------------------|
| **Linux (Arch)** | `sudo pacman -S base-devel webkit2gtk-4.1 gtk3` |
| **Linux (Debian/Ubuntu)** | `sudo apt install build-essential libwebkit2gtk-4.1-dev libgtk-3-dev` |
| **macOS** | Xcode Command Line Tools |
| **Windows** | Visual Studio Build Tools, WebView2 |

### Android

- **Android SDK** API 35
- **NDK** r27+ (`sdkmanager "ndk;27.2.12479018"`)
- **JDK 17** (required — JDK 21+ breaks Gradle)
- **cargo-ndk**: `cargo install cargo-ndk`

## Quick Build (Desktop)

```bash
./run.sh          # Linux/macOS
run.bat           # Windows
```

Manual build:

```bash
cargo build --release -p srltcp-desktop
./target/release/srltcp-desktop
```

Press **Ctrl+C** for graceful shutdown.

## Android Build (Recommended)

```bash
./scripts/build-android.sh
```

This pipeline:

1. Cross-compiles `libsrltcp_core.so` for arm64, armeabi-v7a, x86_64
2. Generates UniFFI Kotlin bindings
3. Builds debug APK with Gradle (Compose BOM `2024.10.01`)
4. Copies APK to `dist/SRLTCPv2-v0.2.1-debug.apk`
5. Runs `scripts/cleanup-android-build.sh` automatically

Install:

```bash
adb install dist/SRLTCPv2-v0.2.1-debug.apk
```

### Manual Android Build

```bash
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/27.2.12479018
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk

cd core
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 \
    -o ../android/app/src/main/jniLibs build --release

cargo run --release --bin uniffi-bindgen -- generate \
    --language kotlin --out-dir ../android/app/src/main/java src/srltcp_core.udl

cd ../android
echo "sdk.dir=$HOME/Android/Sdk" > local.properties
./gradlew assembleDebug
```

### Android Build Cleanup

After building, remove Gradle caches (APK is preserved in `dist/`):

```bash
./scripts/cleanup-android-build.sh
# or
./cleanup.sh --android-build
```

Removes: `android/app/build/`, `android/.gradle/`, `android/build/`

## GitHub Release v0.2.1

```bash
# 1. Build everything
cargo build --release -p srltcp-desktop
./scripts/build-android.sh

# 2. Commit, tag, and publish release with APK
git add -A && git commit -m "Release v0.2.1"
./scripts/create-github-release.sh
```

Or manually:

```bash
git tag -a v0.2.1 -m "SRLTCP v0.2.1"
git push origin main && git push origin v0.2.1
gh release create v0.2.1 \
  --title "SRLTCP v0.2.1" \
  --notes "Release notes here" \
  dist/SRLTCPv2-v0.2.1-debug.apk
```

## Workspace Commands

```bash
cargo check -p srltcp-core
cargo test -p srltcp-core
cargo build --release -p srltcp-desktop
```

## Troubleshooting

| Issue | Solution |
|-------|----------|
| Gradle error `26.0.1` | Set `JAVA_HOME` to JDK 17 |
| Compose BOM resolution fails | Use BOM `2024.10.01` (already set in build.gradle.kts) |
| Android NDK not found | `export ANDROID_NDK_HOME=~/Android/Sdk/ndk/<version>` |
| Port 9473 in use | `./cleanup.sh` or `SRLTCP_PORT=9474 ./run.sh` |
| UniFFI bindgen fails | `cargo build -p srltcp-core` then re-run bindgen |