#!/usr/bin/env bash
# SRLTCP — Full cleanup: kill processes, release ports, remove temp files
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$SCRIPT_DIR/.srltcp.pid"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

log() { echo -e "${BLUE}[cleanup]${NC} $*"; }
ok()  { echo -e "${GREEN}[cleanup]${NC} $*"; }

usage() {
    echo "Usage: ./cleanup.sh [--android-build] [--android-full]"
    echo "  --android-build   Remove Android Gradle caches (keeps dist/*.apk)"
    echo "  --android-full    Also remove jniLibs and UniFFI bindings"
}

ANDROID_BUILD=false
ANDROID_FULL=false
for arg in "$@"; do
    case "$arg" in
        --android-build) ANDROID_BUILD=true ;;
        --android-full) ANDROID_BUILD=true; ANDROID_FULL=true ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $arg"; usage; exit 1 ;;
    esac
done

log "SRLTCP full cleanup starting..."

# Kill by PID file
if [[ -f "$PID_FILE" ]]; then
    pid=$(cat "$PID_FILE")
    if kill -0 "$pid" 2>/dev/null; then
        log "Stopping process $pid (SIGTERM for graceful shutdown)..."
        kill -TERM "$pid" 2>/dev/null || true
        sleep 3
        kill -KILL "$pid" 2>/dev/null || true
    fi
    rm -f "$PID_FILE"
fi

# Kill any remaining srltcp-desktop processes (exact process name only — never editors or
# shells whose command line merely mentions the path)
if pgrep -x "srltcp-desktop" &>/dev/null; then
    log "Stopping remaining srltcp-desktop processes..."
    pkill -TERM -x "srltcp-desktop" 2>/dev/null || true
    sleep 2
    pkill -KILL -x "srltcp-desktop" 2>/dev/null || true
fi
# iroh binds ephemeral UDP ports owned by the process above, so nothing else to release.

# Remove temp/state files
rm -f "$SCRIPT_DIR/.srltcp.pid"
rm -f "$SCRIPT_DIR/.srltcp.log"
rm -rf "$SCRIPT_DIR/.srltcp-tmp" 2>/dev/null || true

if [[ "$ANDROID_BUILD" == true ]]; then
    if [[ "$ANDROID_FULL" == true ]]; then
        "$SCRIPT_DIR/scripts/cleanup-android-build.sh" --full
    else
        "$SCRIPT_DIR/scripts/cleanup-android-build.sh"
    fi
fi

log "Android: use App Info → Force Stop to fully stop the background service."

ok "Cleanup complete. All SRLTCP processes stopped and ports released."