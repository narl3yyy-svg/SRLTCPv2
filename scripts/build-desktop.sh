#!/usr/bin/env bash
# Build desktop binary and stage prebuilt artifact for GitHub Releases
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/version.sh
source "$SCRIPT_DIR/lib/version.sh"

ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
VERSION="$(get_workspace_version "$ROOT_DIR")"
DIST_DIR="$ROOT_DIR/dist"

detect_platform() {
    local os arch
    case "$(uname -s)" in
        Linux) os="linux" ;;
        Darwin) os="macos" ;;
        MINGW*|MSYS*|CYGWIN*) os="windows" ;;
        *) echo "unsupported-os"; return 1 ;;
    esac

    arch="$(uname -m)"
    case "$arch" in
        x86_64|amd64) arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) echo "unsupported-arch: $arch"; return 1 ;;
    esac

    echo "${os}-${arch}"
}

platform="$(detect_platform)"
echo "[desktop] Building SRLTCP v${VERSION} for ${platform}..."

cd "$ROOT_DIR"

if [[ "$(uname -s)" == "Linux" ]]; then
    if ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
        echo "[desktop] ERROR: webkit2gtk-4.1 required. See docs/BUILD.md" >&2
        exit 1
    fi
fi

cargo build --release -p srltcp-desktop

if [[ "$platform" == windows-* ]]; then
    src="$ROOT_DIR/target/release/srltcp-desktop.exe"
    dest_name="srltcp-desktop-${platform}.exe"
else
    src="$ROOT_DIR/target/release/srltcp-desktop"
    dest_name="srltcp-desktop-${platform}"
fi

if [[ ! -f "$src" ]]; then
    echo "[desktop] ERROR: Binary not found at $src" >&2
    exit 1
fi

mkdir -p "$DIST_DIR" "$DIST_DIR/bin/${platform}"
cp -f "$src" "$DIST_DIR/$dest_name"
cp -f "$src" "$DIST_DIR/bin/${platform}/$(basename "$src")"
chmod +x "$DIST_DIR/$dest_name" "$DIST_DIR/bin/${platform}/$(basename "$src")" 2>/dev/null || true

echo "[desktop] ✓ Prebuilt ready:"
echo "  $DIST_DIR/$dest_name"
echo "  $DIST_DIR/bin/${platform}/$(basename "$src")"