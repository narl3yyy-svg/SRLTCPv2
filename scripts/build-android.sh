#!/usr/bin/env bash
# Build SRLTCP Android: native libs + UniFFI bindings + APK + cleanup
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/android-env.sh
source "$SCRIPT_DIR/lib/android-env.sh"

ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CORE_DIR="$ROOT_DIR/core"
ANDROID_DIR="$ROOT_DIR/android"
JNI_DIR="$ANDROID_DIR/app/src/main/jniLibs"
JAVA_OUT="$ANDROID_DIR/app/src/main/java"
DIST_DIR="$ROOT_DIR/dist"
# shellcheck source=lib/version.sh
source "$SCRIPT_DIR/lib/version.sh"
VERSION="$(get_workspace_version "$ROOT_DIR")"
APK_NAME="SRLTCPv2-${VERSION}.apk"

SKIP_NATIVE=false
SKIP_CLEANUP=false

usage() {
    echo "Usage: $0 [--apk-only] [--no-cleanup]"
    echo "  --apk-only     Skip Rust/NDK build (requires existing jniLibs + UniFFI bindings)"
    echo "  --no-cleanup   Keep Gradle caches after build"
}

for arg in "$@"; do
    case "$arg" in
        --apk-only) SKIP_NATIVE=true ;;
        --no-cleanup) SKIP_CLEANUP=true ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $arg"; usage; exit 1 ;;
    esac
done

source "$HOME/.cargo/env" 2>/dev/null || true

echo "══════════════════════════════════════════"
echo "  SRLTCP Android Build v${VERSION}"
echo "══════════════════════════════════════════"

# ── JDK 17 (required) ──────────────────────────────────────────────
if ! find_jdk17; then
    echo "[android] ERROR: JDK 17 is required for Gradle." >&2
    echo "  Install OpenJDK 17 and set JAVA_HOME, e.g.:" >&2
    echo "    Arch:   sudo pacman -S jdk17-openjdk" >&2
    echo "    Debian: sudo apt install openjdk-17-jdk" >&2
    exit 1
fi
echo "[android] JAVA_HOME=$JAVA_HOME"

# ── Android SDK ────────────────────────────────────────────────────
write_local_properties "$ANDROID_DIR"
echo "[android] ANDROID_HOME=$ANDROID_HOME"

if [[ "$SKIP_NATIVE" == false ]]; then
    echo "[android] Building Rust core for Android ABIs..."

    if ! command -v cargo-ndk &>/dev/null; then
        echo "[android] Installing cargo-ndk..."
        cargo install cargo-ndk
    fi

    if ! find_android_ndk; then
        echo "[android] ERROR: Android NDK not found." >&2
        echo "  Install via: sdkmanager \"ndk;27.2.12479018\"" >&2
        echo "  Or set ANDROID_NDK_HOME" >&2
        exit 1
    fi
    echo "[android] ANDROID_NDK_HOME=$ANDROID_NDK_HOME"

    rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android 2>/dev/null || true

    cd "$CORE_DIR"
    cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 \
        -o "$JNI_DIR" build --release

    echo "[android] Generating UniFFI Kotlin bindings..."
    cargo build --release -p srltcp-core
    cargo run --release --bin uniffi-bindgen -- generate \
        --language kotlin \
        --out-dir "$JAVA_OUT" \
        "$CORE_DIR/src/srltcp_core.udl" 2>/dev/null || \
    uniffi-bindgen generate \
        --language kotlin \
        --out-dir "$JAVA_OUT" \
        "$CORE_DIR/src/srltcp_core.udl"
else
    echo "[android] --apk-only: skipping native build"
    if ! verify_jni_libs "$JNI_DIR"; then
        echo "[android] Run without --apk-only to build native libs first." >&2
        exit 1
    fi
    if [[ ! -f "$JAVA_OUT/uniffi/srltcp_core/srltcp_core.kt" ]]; then
        echo "[android] ERROR: UniFFI bindings missing. Run full build first." >&2
        exit 1
    fi
fi

# ── Gradle APK ─────────────────────────────────────────────────────
echo "[android] Building APK with Gradle..."
cd "$ANDROID_DIR"

if [[ ! -x ./gradlew ]]; then
    echo "[android] ERROR: gradlew not found. Run from a complete checkout." >&2
    exit 1
fi

export JAVA_HOME
./gradlew --no-daemon assembleDebug

APK="$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk"
if [[ ! -f "$APK" ]]; then
    echo "[android] ERROR: APK not produced at $APK" >&2
    exit 1
fi

mkdir -p "$DIST_DIR"
cp -f "$APK" "$DIST_DIR/$APK_NAME"
echo "[android] ✓ APK ready: $DIST_DIR/$APK_NAME ($(du -h "$DIST_DIR/$APK_NAME" | cut -f1))"

if [[ "$SKIP_CLEANUP" == false ]]; then
    echo "[android] Cleaning build artifacts..."
    "$SCRIPT_DIR/cleanup-android-build.sh"
fi

echo "[android] Build complete."