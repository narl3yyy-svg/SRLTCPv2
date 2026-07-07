#!/usr/bin/env bash
# Remove Android Gradle build caches after APK is copied to dist/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ANDROID_DIR="$ROOT_DIR/android"
DIST_DIR="$ROOT_DIR/dist"

BLUE='\033[0;34m'
GREEN='\033[0;32m'
NC='\033[0m'

log() { echo -e "${BLUE}[cleanup-android]${NC} $*"; }
ok()  { echo -e "${GREEN}[cleanup-android]${NC} $*"; }

log "Cleaning Android build artifacts..."

# Preserve APK in dist/ if present
if [[ -f "$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk" ]]; then
    mkdir -p "$DIST_DIR"
    cp -f "$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk" \
        "$DIST_DIR/SRLTCPv2-v0.2.1-debug.apk"
    ok "APK preserved at dist/SRLTCPv2-v0.2.1-debug.apk"
fi

# Remove large Gradle/R build directories
rm -rf "$ANDROID_DIR/app/build"
rm -rf "$ANDROID_DIR/.gradle"
rm -rf "$ANDROID_DIR/build"

ok "Removed android/app/build, android/.gradle, android/build"
log "Native libs (jniLibs/) and source are kept for incremental rebuilds."
log "Run scripts/build-android.sh to rebuild from scratch."