#!/usr/bin/env bash
# Remove Android Gradle build caches after APK is copied to dist/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ANDROID_DIR="$ROOT_DIR/android"
DIST_DIR="$ROOT_DIR/dist"
# shellcheck source=lib/version.sh
source "$(dirname "$0")/lib/version.sh"
VERSION="$(get_workspace_version "$(cd "$(dirname "$0")/.." && pwd)")"
APK_NAME="SRLTCPv2-${VERSION}.apk"

FULL_CLEAN=false
for arg in "$@"; do
    case "$arg" in
        --full) FULL_CLEAN=true ;;
    esac
done

BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

log() { echo -e "${BLUE}[cleanup-android]${NC} $*"; }
ok()  { echo -e "${GREEN}[cleanup-android]${NC} $*"; }
warn() { echo -e "${YELLOW}[cleanup-android]${NC} $*"; }

log "Cleaning Android build artifacts..."

# Preserve APK in dist/ if present in build output
if [[ -f "$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk" ]]; then
    mkdir -p "$DIST_DIR"
    cp -f "$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk" \
        "$DIST_DIR/$APK_NAME"
    ok "APK preserved at dist/$APK_NAME"
elif [[ -f "$DIST_DIR/$APK_NAME" ]]; then
    ok "APK already in dist/$APK_NAME"
else
    warn "No APK found to preserve"
fi

# Remove Gradle build caches
rm -rf "$ANDROID_DIR/app/build"
rm -rf "$ANDROID_DIR/.gradle"
rm -rf "$ANDROID_DIR/build"

ok "Removed android/app/build, android/.gradle, android/build"

if [[ "$FULL_CLEAN" == true ]]; then
    rm -rf "$ANDROID_DIR/app/src/main/jniLibs"
    rm -rf "$ANDROID_DIR/app/src/main/java/uniffi"
    warn "Removed jniLibs/ and UniFFI bindings (--full)"
fi

log "Kept: source code, gradle wrapper, jniLibs/ (unless --full)"
log "Rebuild: ./scripts/build-android.sh  |  APK only: ./scripts/assemble-apk.sh"