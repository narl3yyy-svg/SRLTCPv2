# Build Instructions

Building SRLTCP v0.2.0 on all platforms.

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
./run.sh          # Linux/macOS — builds on first run, launches app
run.bat           # Windows
```

Manual build:

```bash
cargo build --release -p srltcp-desktop
./target/release/srltcp-desktop    # Linux/macOS
```

Press **Ctrl+C** for graceful shutdown. The Rust core calls `shutdown()` to release serial ports and QUIC listeners.

## Android Build (Recommended)

Use the all-in-one script:

```bash
./scripts/build-android.sh
```

This will:

1. Cross-compile `libsrltcp_core.so` for arm64, armeabi-v7a, x86_64
2. Generate UniFFI Kotlin bindings
3. Build the debug APK with Gradle
4. Copy the APK to `dist/SRLTCPv2-debug.apk`
5. Clean up Gradle caches (`android/app/build/`, `android/.gradle/`)

Install on device:

```bash
adb install dist/SRLTCPv2-debug.apk
```

### Manual Android Build

```bash
# 1. Native libs
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/27.2.12479018
cd core
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 \
    -o ../android/app/src/main/jniLibs build --release

# 2. UniFFI bindings
cargo run --release --bin uniffi-bindgen -- generate \
    --language kotlin \
    --out-dir ../android/app/src/main/java \
    src/srltcp_core.udl

# 3. APK (requires JDK 17)
cd ../android
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk   # adjust for your system
echo "sdk.dir=$HOME/Android/Sdk" > local.properties
./gradlew assembleDebug
```

### Android Build Cleanup

After building, remove large Gradle caches:

```bash
./scripts/cleanup-android-build.sh
# or
./cleanup.sh --android-build
```

This preserves the APK in `dist/SRLTCPv2-debug.apk` and removes:

- `android/app/build/`
- `android/.gradle/`
- `android/build/`

Native libs in `android/app/src/main/jniLibs/` are kept for faster incremental rebuilds.

## Workspace Commands

```bash
cargo check -p srltcp-core          # Fast compile check
cargo build -p srltcp-core          # Core library only
cargo build -p srltcp-desktop       # Desktop app
cargo test -p srltcp-core           # Run core tests
```

## Troubleshooting

| Issue | Solution |
|-------|----------|
| `cargo not found` | Run `run.sh` (auto-installs) or install rustup |
| Tauri webkit error (Linux) | Install `webkit2gtk-4.1` dev package |
| Gradle error `26.0.1` | Set `JAVA_HOME` to JDK 17 |
| Android NDK not found | `export ANDROID_NDK_HOME=~/Android/Sdk/ndk/<version>` |
| Port 9473 in use | `SRLTCP_PORT=9474 ./run.sh` or `./cleanup.sh` |
| UniFFI bindgen fails | `cargo build -p srltcp-core && cargo run --bin uniffi-bindgen ...` |

## Release Builds

```bash
# Desktop
cargo build --release -p srltcp-desktop

# Android
cd android && JAVA_HOME=/path/to/jdk-17 ./gradlew assembleRelease
```