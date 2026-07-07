#!/usr/bin/env bash
# SRLTCP v0.2.1 — Download and Run (Linux/macOS)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PID_FILE="$SCRIPT_DIR/.srltcp.pid"
LOG_FILE="$SCRIPT_DIR/.srltcp.log"
QUIC_PORT="${SRLTCP_PORT:-9473}"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[SRLTCP]${NC} $*"; }
ok()   { echo -e "${GREEN}[SRLTCP]${NC} $*"; }
err()  { echo -e "${RED}[SRLTCP]${NC} $*" >&2; }

cleanup() {
    log "Shutting down gracefully (Ctrl+C)..."
    if [[ -f "$PID_FILE" ]]; then
        local pid
        pid=$(cat "$PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            # SIGTERM triggers Rust shutdown() via signal-hook (serial, QUIC, peers)
            kill -TERM "$pid" 2>/dev/null || true
            local waited=0
            while kill -0 "$pid" 2>/dev/null && [[ $waited -lt 20 ]]; do
                sleep 0.5
                waited=$((waited + 1))
            done
            if kill -0 "$pid" 2>/dev/null; then
                warn "Process did not exit cleanly, sending SIGKILL"
                kill -KILL "$pid" 2>/dev/null || true
            fi
        fi
        rm -f "$PID_FILE"
    fi
    # Release QUIC port if still held
    if command -v fuser &>/dev/null; then
        fuser -k "${QUIC_PORT}/udp" 2>/dev/null || true
        fuser -k "${QUIC_PORT}/tcp" 2>/dev/null || true
    fi
    ok "Shutdown complete — ports and resources released."
    exit 0
}

warn() { echo -e "${RED}[SRLTCP]${NC} $*" >&2; }

trap cleanup SIGINT SIGTERM

# ── Ensure Rust toolchain ──────────────────────────────────────────
ensure_rust() {
    if [[ -f "$HOME/.cargo/env" ]]; then
        # shellcheck source=/dev/null
        source "$HOME/.cargo/env"
    fi
    if ! command -v cargo &>/dev/null; then
        log "Rust not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
        # shellcheck source=/dev/null
        source "$HOME/.cargo/env"
    fi
    ok "Rust $(rustc --version | awk '{print $2}')"
}

# ── Ensure Tauri system dependencies (Linux) ───────────────────────
ensure_system_deps() {
    if [[ "$(uname)" == "Linux" ]]; then
        local missing=()
        for cmd in pkg-config gcc; do
            command -v "$cmd" &>/dev/null || missing+=("$cmd")
        done
        if [[ ${#missing[@]} -gt 0 ]]; then
            log "Some build tools may be missing: ${missing[*]}"
            log "On Arch:   sudo pacman -S base-devel webkit2gtk-4.1 gtk3"
            log "On Debian: sudo apt install build-essential libwebkit2gtk-4.1-dev libgtk-3-dev"
        fi
    fi
}

# ── Build if needed ────────────────────────────────────────────────
build_if_needed() {
    local binary="target/release/srltcp-desktop"
    if [[ ! -f "$binary" ]]; then
        log "First run — building SRLTCP (this may take a few minutes)..."
        ensure_system_deps
        cargo build --release -p srltcp-desktop 2>&1 | tee -a "$LOG_FILE"
        ok "Build complete."
    else
        log "Binary found — skipping build. Run 'cargo build --release -p srltcp-desktop' to rebuild."
    fi
}

# ── Check for stale process ────────────────────────────────────────
check_stale() {
    if [[ -f "$PID_FILE" ]]; then
        local old_pid
        old_pid=$(cat "$PID_FILE")
        if kill -0 "$old_pid" 2>/dev/null; then
            err "SRLTCP already running (PID $old_pid). Run ./cleanup.sh first."
            exit 1
        fi
        rm -f "$PID_FILE"
    fi
}

# ── Launch ─────────────────────────────────────────────────────────
main() {
    echo ""
    echo "  ╔══════════════════════════════════════╗"
    echo "  ║       SRLTCP v0.2.1 — Desktop        ║"
    echo "  ║   Secure P2P over Serial/LAN/WAN     ║"
    echo "  ╚══════════════════════════════════════╝"
    echo ""

    ensure_rust
    check_stale
    build_if_needed

    local binary="target/release/srltcp-desktop"
    if [[ ! -f "$binary" ]]; then
        # Fallback: run via cargo
        log "Launching via cargo run..."
        RUST_LOG="${RUST_LOG:-info}" cargo run --release -p srltcp-desktop &
    else
        log "Launching SRLTCP..."
        RUST_LOG="${RUST_LOG:-info}" "$binary" &
    fi

    local app_pid=$!
    echo "$app_pid" > "$PID_FILE"
    ok "SRLTCP started (PID $app_pid, QUIC port $QUIC_PORT)"
    log "Press Ctrl+C to shut down gracefully."
    echo ""

    wait "$app_pid" || true
    cleanup
}

main "$@"