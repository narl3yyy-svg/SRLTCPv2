#!/usr/bin/env bash
# Create GitHub Release v0.2.1 with debug APK attached
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
VERSION="v0.2.1"
APK="$ROOT_DIR/dist/SRLTCPv2-v0.2.1-debug.apk"
REPO="narl3yyy-svg/SRLTCPv2"

cd "$ROOT_DIR"

if [[ ! -f "$APK" ]]; then
    echo "[release] APK not found. Run ./scripts/build-android.sh first."
    exit 1
fi

if ! command -v gh &>/dev/null; then
    echo "[release] ERROR: GitHub CLI (gh) is required."
    exit 1
fi

echo "[release] Pushing commits and tag $VERSION..."
git push origin main
git tag -a "$VERSION" -m "SRLTCP $VERSION" 2>/dev/null || true
git push origin "$VERSION"

echo "[release] Creating GitHub Release..."
gh release create "$VERSION" \
    --repo "$REPO" \
    --title "SRLTCP $VERSION" \
    --notes "## SRLTCP $VERSION

### Highlights
- Stable Compose BOM for Android builds
- Desktop UI: peers, messaging, file transfer, voice/video calls
- Android foreground service with file transfer and call UI
- Graceful shutdown in run.sh / run.bat
- Build artifact cleanup scripts

### Install
\`\`\`bash
# Desktop
git clone https://github.com/$REPO.git && cd SRLTCPv2 && ./run.sh

# Android
adb install SRLTCPv2-v0.2.1-debug.apk
\`\`\`" \
    "$APK"

echo "[release] Done: https://github.com/$REPO/releases/tag/$VERSION"