#!/usr/bin/env bash
# SRLTCP v0.2.3 — Download and Run (Linux/macOS)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PID_FILE="$SCRIPT_DIR/.srltcp.pid"
LOG_FILE="$SCRIPT_DIR/.srltcp.log"
QUIC_PORT="${SRLTCP_PORT:-9473}"
VERSION="0.2.3"
REPO="narl3yyy-svg/SRLTCPv2"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m'

log()  { echo -e "${BLUE}[SRLTCP]${NC} $*"; }
ok()   { echo -e "${GREEN}[SRLTCP]${NC} $*"; }
warn() { echo -e "${YELLOW}[SRLTCP]${NC} $*"; }
err()  { echo -e "${RED}[SRLTCP]${NC} $*" >&2; }

FORCE_REBUILD=false
USE_PREBUILT=true

usage() {
    echo "Usage: $0 [--rebuild] [--no-prebuilt]"
    echo "  --rebuild      Force recompile from source"
    echo "  --no-prebuilt  Skip prebuilt binary lookup"
}

for arg in "$@"; do
    case "$arg" in
        --rebuild) FORCE_REBUILD=true ;;
        --no-prebuilt) USE_PREBUILT=false ;;
        -h|--help) usage; exit 0 ;;
        *) err "Unknown option: $arg"; usage; exit 1 ;;
    esac
done

cleanup() {
    log "Shutting down gracefully (Ctrl+C)..."
    if [[ -f "$PID_FILE" ]]; then
        local pid
        pid=$(cat "$PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
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
    if command -v fuser &>/dev/null; then
        fuser -k "${QUIC_PORT}/udp" 2>/dev/null || true
        fuser -k "${QUIC_PORT}/tcp" 2>/dev/null || true
    fi
    ok "Shutdown complete — ports and resources released."
    exit 0
}

trap cleanup SIGINT SIGTERM

detect_os() {
    case "$(uname -s)" in
        Linux)
            if [[ -f /etc/os-release ]]; then
                # shellcheck source=/dev/null
                . /etc/os-release
                echo "${ID:-linux}"
            else
                echo "linux"
            fi
            ;;
        Darwin) echo "macos" ;;
        *) echo "unknown" ;;
    esac
}

detect_arch() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64|amd64) echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *) echo "$arch" ;;
    esac
}

platform_tag() {
    local os arch
    os="$(detect_os)"
    arch="$(detect_arch)"
    if [[ "$os" == "macos" ]]; then
        echo "macos-${arch}"
    else
        echo "linux-${arch}"
    fi
}

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

check_linux_deps() {
    local os missing=()
    os="$(detect_os)"

    if [[ "$(uname -s)" != "Linux" ]]; then
        return 0
    fi

    for cmd in pkg-config gcc; do
        command -v "$cmd" &>/dev/null || missing+=("$cmd")
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        warn "Missing build tools: ${missing[*]}"
        case "$os" in
            ubuntu|debian|pop|linuxmint)
                warn "Install with:"
                echo "  sudo apt update && sudo apt install -y build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf"
                ;;
            arch|manjaro|endeavouros)
                warn "Install with:"
                echo "  sudo pacman -S --needed base-devel webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg patchelf"
                ;;
            fedora)
                warn "Install with:"
                echo "  sudo dnf install webkit2gtk4.1-devel gtk3-devel librsvg2-devel openssl-devel"
                ;;
            *)
                warn "Install webkit2gtk-4.1, gtk3, and base build tools for your distro."
                ;;
        esac
        return 1
    fi

    if ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
        warn "webkit2gtk-4.1 not found (required for Tauri on Linux)"
        case "$os" in
            ubuntu|debian|pop|linuxmint)
                echo "  sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev"
                ;;
            arch|manjaro|endeavouros)
                echo "  sudo pacman -S --needed webkit2gtk-4.1 gtk3"
                ;;
        esac
        return 1
    fi
    return 0
}

check_macos_deps() {
    if [[ "$(uname -s)" != "Darwin" ]]; then
        return 0
    fi
    if ! xcode-select -p &>/dev/null; then
        warn "Xcode Command Line Tools not found."
        echo "  xcode-select --install"
        return 1
    fi
    return 0
}

find_binary() {
    local platform built prebuilt cached

    built="target/release/srltcp-desktop"
    platform="$(platform_tag)"
    prebuilt="dist/bin/${platform}/srltcp-desktop"
    cached="dist/bin/srltcp-desktop"

    if [[ "$FORCE_REBUILD" == true ]]; then
        echo ""
        return 1
    fi

    if [[ "$USE_PREBUILT" == true ]]; then
        if [[ -f "$prebuilt" && -x "$prebuilt" ]]; then
            echo "$prebuilt"
            return 0
        fi
        if [[ -f "$cached" && -x "$cached" ]]; then
            echo "$cached"
            return 0
        fi
    fi

    if [[ -f "$built" && -x "$built" ]]; then
        echo "$built"
        return 0
    fi

    echo ""
    return 1
}

download_prebuilt() {
    local platform dest dir url
    platform="$(platform_tag)"
    dir="dist/bin/${platform}"
    dest="${dir}/srltcp-desktop"

    if [[ "$USE_PREBUILT" != true ]] || [[ "$FORCE_REBUILD" == true ]]; then
        return 1
    fi

    if ! command -v curl &>/dev/null; then
        return 1
    fi

    mkdir -p "$dir"
    url="https://github.com/${REPO}/releases/download/v${VERSION}/srltcp-desktop-${platform}"

    log "Trying prebuilt binary for ${platform}..."
    if curl -fsSL --retry 2 -o "$dest" "$url" 2>/dev/null; then
        chmod +x "$dest"
        ok "Downloaded prebuilt: $dest"
        echo "$dest"
        return 0
    fi

    rm -f "$dest"
    return 1
}

build_from_source() {
    log "Building from source (first run may take several minutes)..."
    if [[ "$(uname -s)" == "Linux" ]]; then
        check_linux_deps || warn "Some dependencies missing — build may fail."
    elif [[ "$(uname -s)" == "Darwin" ]]; then
        check_macos_deps || warn "Xcode tools missing — build may fail."
    fi
    cargo build --release -p srltcp-desktop 2>&1 | tee -a "$LOG_FILE"
    ok "Build complete."
}

resolve_binary() {
    local bin
    bin="$(find_binary)"
    if [[ -n "$bin" ]]; then
        echo "$bin"
        return 0
    fi

    bin="$(download_prebuilt)" || true
    if [[ -n "$bin" ]]; then
        echo "$bin"
        return 0
    fi

    ensure_rust
    build_from_source
    echo "target/release/srltcp-desktop"
}

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

main() {
    echo ""
    echo "  ╔══════════════════════════════════════╗"
    echo "  ║       SRLTCP v${VERSION} — Desktop        ║"
    echo "  ║   Secure P2P over Serial/LAN/WAN     ║"
    echo "  ╚══════════════════════════════════════╝"
    echo ""

    local os arch
    os="$(detect_os)"
    arch="$(detect_arch)"
    log "Platform: ${os} / ${arch}"

    check_stale

    local binary
    binary="$(resolve_binary)"

    if [[ ! -f "$binary" ]]; then
        err "Binary not found at $binary"
        exit 1
    fi

    log "Launching: $binary"
    RUST_LOG="${RUST_LOG:-info}" "$binary" &
    local app_pid=$!
    echo "$app_pid" > "$PID_FILE"
    ok "SRLTCP started (PID $app_pid, QUIC port $QUIC_PORT)"
    log "Press Ctrl+C or close the window to shut down."
    echo ""

    wait "$app_pid" || true
    cleanup
}

main "$@"