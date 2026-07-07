#!/usr/bin/env bash
# Create GitHub Release with debug APK attached
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO="narl3yyy-svg/SRLTCPv2"

VERSION=$(grep '^version' "$ROOT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
TAG="v${VERSION}"
APK="$ROOT_DIR/dist/SRLTCPv2-v${VERSION}-debug.apk"

cd "$ROOT_DIR"

if [[ ! -f "$APK" ]]; then
    echo "[release] APK not found at dist/SRLTCPv2-v${VERSION}-debug.apk"
    echo "[release] Run: ./scripts/build-android.sh"
    exit 1
fi

if ! command -v gh &>/dev/null; then
    echo "[release] ERROR: GitHub CLI (gh) required."
    exit 1
fi

echo "[release] Pushing main and tag $TAG..."
git push origin main
git tag -a "$TAG" -m "SRLTCP $TAG" 2>/dev/null || git tag -f -a "$TAG" -m "SRLTCP $TAG"
git push origin "$TAG" --force

echo "[release] Creating GitHub Release $TAG..."
gh release create "$TAG" \
    --repo "$REPO" \
    --title "SRLTCP $TAG" \
    --notes "## SRLTCP $TAG

### Install
\`\`\`bash
# Desktop
git clone https://github.com/$REPO.git && cd SRLTCPv2 && ./run.sh

# Android
adb install SRLTCPv2-v${VERSION}-debug.apk
\`\`\`

See [BUILD.md](docs/BUILD.md) for full build instructions." \
    "$APK" 2>/dev/null || \
gh release upload "$TAG" "$APK" --repo "$REPO" --clobber

echo "[release] Done: https://github.com/$REPO/releases/tag/$TAG"