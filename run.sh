#!/usr/bin/env bash
# SRLTCP — Download and Run (Linux/macOS)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# shellcheck source=scripts/lib/version.sh
source "$SCRIPT_DIR/scripts/lib/version.sh"

PID_FILE="$SCRIPT_DIR/.srltcp.pid"
LOG_FILE="$SCRIPT_DIR/.srltcp.log"
QUIC_PORT="${SRLTCP_PORT:-9473}"
VERSION="$(get_workspace_version "$SCRIPT_DIR")"
REPO="narl3yyy-svg/SRLTCPv2"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m'

log()  { echo -e "${BLUE}[SRLTCP]${NC} $*" >&2; }
ok()   { echo -e "${GREEN}[SRLTCP]${NC} $*" >&2; }
warn() { echo -e "${YELLOW}[SRLTCP]${NC} $*" >&2; }
err()  { echo -e "${RED}[SRLTCP]${NC} $*" >&2; }

FORCE_REBUILD=false
USE_PREBUILT=true
GIT_PULL=false
PREBUILT_RETRIES="${SRLTCP_PREBUILT_RETRIES:-18}"
PREBUILT_RETRY_SECS="${SRLTCP_PREBUILT_RETRY_SECS:-10}"

usage() {
    echo "Usage: $0 [--pull] [--rebuild] [--no-prebuilt]"
    echo "  (default)      Launch prebuilt binary (local or downloaded from Releases)"
    echo "  --pull         git pull --ff-only from origin/main before launch"
    echo "  --rebuild      Compile from source, then launch (developers only)"
    echo "  --no-prebuilt  Skip GitHub download (use local dist/ only)"
    echo ""
    echo "Prebuilt download retries: ${PREBUILT_RETRIES}x every ${PREBUILT_RETRY_SECS}s while CI publishes."
}

for arg in "$@"; do
    case "$arg" in
        --pull) GIT_PULL=true ;;
        --rebuild) FORCE_REBUILD=true ;;
        --no-prebuilt) USE_PREBUILT=false ;;
        -h|--help) usage; exit 0 ;;
        *) err "Unknown option: $arg"; usage; exit 1 ;;
    esac
done

maybe_git_pull() {
    if [[ "$GIT_PULL" != true ]]; then
        return 0
    fi
    if [[ ! -d "$SCRIPT_DIR/.git" ]] || ! command -v git &>/dev/null; then
        warn "git pull skipped — not a git checkout"
        return 0
    fi
    log "Updating from origin/main (git pull --ff-only)..."
    if git -C "$SCRIPT_DIR" pull --ff-only origin main; then
        VERSION="$(get_workspace_version "$SCRIPT_DIR")"
        ok "Updated to v${VERSION}"
    else
        warn "git pull failed — continuing with local tree"
    fi
}

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
    if [[ "$(uname -s)" == "Linux" ]] && command -v fuser &>/dev/null; then
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
                echo "  sudo apt update && sudo apt install -y build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libudev-dev"
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

prebuilt_version_file() {
    local platform="$1"
    echo "dist/bin/${platform}/.prebuilt-version"
}

cached_prebuilt_version() {
    local vf
    vf="$(prebuilt_version_file "$(platform_tag)")"
    [[ -f "$vf" ]] && cat "$vf" || echo ""
}

mark_prebuilt_version() {
    local platform="$1" bin="$2"
    mkdir -p "dist/bin/${platform}"
    echo "$VERSION" > "$(prebuilt_version_file "$platform")"
    log "Cached prebuilt v${VERSION} at $bin"
}

validate_binary_file() {
    local bin="$1"
    [[ -f "$bin" ]] || return 1
    [[ -s "$bin" ]] || return 1
    [[ -x "$bin" ]] || chmod +x "$bin" 2>/dev/null || true
    local size
    size=$(stat -c%s "$bin" 2>/dev/null || stat -f%z "$bin" 2>/dev/null || echo 0)
    [[ "$size" -gt 1048576 ]] || return 1
    return 0
}

is_prebuilt_current() {
    local cached
    cached="$(cached_prebuilt_version)"
    [[ "$cached" == "$VERSION" ]]
}

validate_binary() {
    validate_binary_file "$1" && is_prebuilt_current
}

find_binary() {
    local platform candidates=()

    platform="$(platform_tag)"

    if [[ "$FORCE_REBUILD" == true ]]; then
        echo ""
        return 1
    fi

    candidates=(
        "dist/bin/${platform}/srltcp-desktop"
        "dist/srltcp-desktop-${platform}"
        "dist/bin/srltcp-desktop"
        "target/release/srltcp-desktop"
    )

    local candidate
    for candidate in "${candidates[@]}"; do
        if validate_binary_file "$candidate"; then
            if is_prebuilt_current; then
                printf '%s' "$candidate"
                return 0
            fi
            warn "Stale prebuilt at $candidate (have v$(cached_prebuilt_version), need v${VERSION})"
        fi
    done

    echo ""
    return 1
}

http_download() {
    local url="$1" dest="$2"
    if command -v curl &>/dev/null; then
        curl -fsSL --retry 3 --connect-timeout 15 -o "$dest" "$url"
        return $?
    fi
    if command -v wget &>/dev/null; then
        wget -q --tries=3 --timeout=15 -O "$dest" "$url"
        return $?
    fi
    return 1
}

download_prebuilt_once() {
    local tag_version="$1"
    local platform dest dir url
    platform="$(platform_tag)"
    dir="dist/bin/${platform}"
    dest="${dir}/srltcp-desktop"

    mkdir -p "$dir"
    url="https://github.com/${REPO}/releases/download/v${tag_version}/srltcp-desktop-${platform}"

    if http_download "$url" "$dest" 2>/dev/null && validate_binary_file "$dest"; then
        chmod +x "$dest"
        mark_prebuilt_version "$platform" "$dest"
        ok "Downloaded prebuilt v${tag_version}: $dest"
        printf '%s' "$dest"
        return 0
    fi

    rm -f "$dest"
    return 1
}

download_prebuilt() {
    local platform attempt bin
    platform="$(platform_tag)"

    if [[ "$USE_PREBUILT" != true ]] || [[ "$FORCE_REBUILD" == true ]]; then
        return 1
    fi

    if ! command -v curl &>/dev/null && ! command -v wget &>/dev/null; then
        warn "curl or wget required to download prebuilt binaries."
        return 1
    fi

    log "Downloading prebuilt for ${platform} (v${VERSION})..."
    for ((attempt = 1; attempt <= PREBUILT_RETRIES; attempt++)); do
        bin="$(download_prebuilt_once "$VERSION")" && return 0
        if [[ "$attempt" -lt "$PREBUILT_RETRIES" ]]; then
            warn "Prebuilt v${VERSION} not published yet (${attempt}/${PREBUILT_RETRIES}) — CI may still be running..."
            warn "Check: https://github.com/${REPO}/actions — retrying in ${PREBUILT_RETRY_SECS}s"
            sleep "$PREBUILT_RETRY_SECS"
        fi
    done

    warn "Prebuilt not available for ${platform} at v${VERSION} after ${PREBUILT_RETRIES} attempts."
    return 1
}

build_from_source() {
    log "Building from source (first run may take several minutes)..."
    if [[ "$(uname -s)" == "Linux" ]]; then
        check_linux_deps || warn "Some dependencies missing — build may fail."
    elif [[ "$(uname -s)" == "Darwin" ]]; then
        check_macos_deps || warn "Xcode tools missing — build may fail."
    fi
    # Must not write to stdout — resolve_binary captures stdout as the binary path.
    cargo build --release -p srltcp-desktop 2>&1 | tee -a "$LOG_FILE" >&2
    ok "Build complete."
}

# Return first valid binary on disk (ignores --rebuild flag; used after local compile).
find_staged_binary() {
    local platform candidates=()
    platform="$(platform_tag)"
    candidates=(
        "dist/bin/${platform}/srltcp-desktop"
        "dist/srltcp-desktop-${platform}"
        "target/release/srltcp-desktop"
    )
    local candidate
    for candidate in "${candidates[@]}"; do
        if validate_binary_file "$candidate"; then
            printf '%s' "$candidate"
            return 0
        fi
    done
    echo ""
    return 1
}

resolve_binary() {
    local bin platform
    platform="$(platform_tag)"

    if [[ "$FORCE_REBUILD" == true ]]; then
        ensure_rust
        if [[ -x "$SCRIPT_DIR/scripts/build-desktop.sh" ]]; then
            "$SCRIPT_DIR/scripts/build-desktop.sh" >&2 || {
                err "Build failed. See docs/BUILD.md for dependencies."
                exit 1
            }
        else
            build_from_source
            local platform
            platform="$(platform_tag)"
            mkdir -p "dist/bin/${platform}"
            cp -f target/release/srltcp-desktop "dist/bin/${platform}/srltcp-desktop" 2>/dev/null || true
            echo "$VERSION" > "$(prebuilt_version_file "$platform")" 2>/dev/null || true
        fi
        bin="$(find_staged_binary)"
        if [[ -n "$bin" ]]; then
            printf '%s' "$bin"
            return 0
        fi
        err "Build finished but binary not found — check target/release/ and dist/"
        exit 1
    fi

    bin="$(find_binary)"
    if [[ -n "$bin" ]]; then
        printf '%s' "$bin"
        return 0
    fi

    bin="$(download_prebuilt)" || true
    if [[ -n "$bin" ]] && validate_binary_file "$bin"; then
        printf '%s' "$bin"
        return 0
    fi

    warn "No GitHub prebuilt for v${VERSION} yet — building locally..."
    if [[ -x "$SCRIPT_DIR/scripts/build-desktop.sh" ]]; then
        "$SCRIPT_DIR/scripts/build-desktop.sh" >&2 || {
            err "Local build failed. Install deps (Rust, webkit2gtk) or wait for CI:"
            err "  https://github.com/${REPO}/releases/tag/v${VERSION}"
            exit 1
        }
        bin="$(find_staged_binary)"
        if [[ -n "$bin" ]]; then
            printf '%s' "$bin"
            return 0
        fi
    fi

    err "No binary available for ${platform} at v${VERSION}."
    err "Try: ./run.sh --rebuild  or  ./scripts/build-desktop.sh"
    exit 1
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
    maybe_git_pull

    local binary
    binary="$(resolve_binary)"

    if ! validate_binary_file "$binary"; then
        err "Binary missing or invalid."
        err "  Path tried: ${binary:-<empty>}"
        err "  Expected: dist/bin/$(platform_tag)/srltcp-desktop (v${VERSION})"
        err "Try: $0 --rebuild   or   ./scripts/build-desktop.sh"
        exit 1
    fi

    log "Launching: $binary"
    # Suppress harmless WebKit spellcheck (libenchant) plugin warnings on Linux
    export ENCHANT_MODULE_DIR="${ENCHANT_MODULE_DIR:-/dev/null}"
    export G_MESSAGES_DEBUG="${G_MESSAGES_DEBUG:-}"
    if [[ "$(uname -s)" == "Linux" ]]; then
        # WebKitGTK media capture via xdg-desktop-portal + PipeWire
        export GTK_USE_PORTAL="${GTK_USE_PORTAL:-1}"
        export PIPEWIRE_RUNTIME_DIR="${PIPEWIRE_RUNTIME_DIR:-${XDG_RUNTIME_DIR:-/run/user/$(id -u)}}"
        # Allow portal permission prompts for mic/camera in the webview
        export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS="${WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS:-1}"
        # Reduce GStreamer GstIntRange noise from WebKit device probing
        export GST_DEBUG="${GST_DEBUG:-*:0}"
    fi
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