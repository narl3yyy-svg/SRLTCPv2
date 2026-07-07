#!/usr/bin/env bash
# Build SRLTCP Android native library + UniFFI Kotlin bindings + APK
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CORE_DIR="$ROOT_DIR/core"
ANDROID_DIR="$ROOT_DIR/android"
JNI_DIR="$ANDROID_DIR/app/src/main/jniLibs"
JAVA_OUT="$ANDROID_DIR/app/src/main/java"
DIST_DIR="$ROOT_DIR/dist"

source "$HOME/.cargo/env" 2>/dev/null || true

echo "[android] Building Rust core for Android ABIs..."

if ! command -v cargo-ndk &>/dev/null; then
    echo "[android] Installing cargo-ndk..."
    cargo install cargo-ndk
fi

rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android 2>/dev/null || true

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    for ndk in "$HOME/Android/Sdk/ndk/"*; do
        if [[ -d "$ndk" ]]; then
            export ANDROID_NDK_HOME="$ndk"
            break
        fi
    done
fi

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    echo "[android] ERROR: Set ANDROID_NDK_HOME to your Android NDK path"
    exit 1
fi

echo "[android] Using NDK: $ANDROID_NDK_HOME"

cd "$CORE_DIR"
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 \
    -o "$JNI_DIR" build --release

echo "[android] Generating UniFFI Kotlin bindings..."
cargo build --release
cargo run --release --bin uniffi-bindgen -- generate \
    --language kotlin \
    --out-dir "$JAVA_OUT" \
    "$CORE_DIR/src/srltcp_core.udl" 2>/dev/null || \
uniffi-bindgen generate \
    --language kotlin \
    --out-dir "$JAVA_OUT" \
    "$CORE_DIR/src/srltcp_core.udl"

echo "[android] Building APK..."
cd "$ANDROID_DIR"

# Android Gradle Plugin requires JDK 17 (not JDK 21+)
if [[ -z "${JAVA_HOME:-}" ]]; then
    for jdk in /usr/lib/jvm/java-17-openjdk "$HOME/.jdks/temurin-17"; do
        if [[ -d "$jdk" ]]; then
            export JAVA_HOME="$jdk"
            break
        fi
    done
fi

if [[ -z "${JAVA_HOME:-}" ]]; then
    echo "[android] WARN: JAVA_HOME not set — Gradle may fail on JDK 21+"
fi

if [[ -z "${ANDROID_HOME:-}" ]]; then
    export ANDROID_HOME="${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}"
fi
if [[ ! -f local.properties ]]; then
    echo "sdk.dir=$ANDROID_HOME" > local.properties
fi

if [[ -x ./gradlew ]]; then
    ./gradlew assembleDebug
else
    gradle assembleDebug
fi

APK="$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk"
if [[ -f "$APK" ]]; then
    mkdir -p "$DIST_DIR"
    cp -f "$APK" "$DIST_DIR/SRLTCPv2-debug.apk"
    echo "[android] APK ready: $DIST_DIR/SRLTCPv2-debug.apk"
    echo "[android] Cleaning build artifacts..."
    "$SCRIPT_DIR/cleanup-android-build.sh"
else
    echo "[android] Build finished — check android/app/build/outputs/apk/"
    exit 1
fi