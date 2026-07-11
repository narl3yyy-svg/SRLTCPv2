#!/usr/bin/env bash
# Create GitHub Release — desktop prebuilts + Android APK
# Prefer pushing tag v* to trigger .github/workflows/release.yml (CI builds all platforms).
# This script is a manual fallback when artifacts are already in dist/.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO="narl3yyy-svg/SRLTCPv2"

# shellcheck source=lib/version.sh
source "$SCRIPT_DIR/lib/version.sh"

VERSION="$(get_workspace_version "$ROOT_DIR")"
TAG="v${VERSION}"
APK="$ROOT_DIR/dist/SRLTCPv2-${VERSION}.apk"

cd "$ROOT_DIR"

if ! command -v gh &>/dev/null; then
    echo "[release] ERROR: GitHub CLI (gh) required."
    exit 1
fi

echo "[release] Recommended: push tag to trigger CI release workflow"
echo "  git push origin main"
echo "  git tag -a $TAG -m \"SRLTCP $TAG\""
echo "  git push origin $TAG"
echo ""
read -r -p "Continue with manual release upload from dist/? [y/N] " confirm
if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
    echo "[release] Aborted. Use CI by pushing tag $TAG instead."
    exit 0
fi

assets=()
for name in \
    "srltcp-desktop-linux-x86_64" \
    "srltcp-desktop-linux-aarch64" \
    "srltcp-desktop-macos-x86_64" \
    "srltcp-desktop-macos-aarch64" \
    "srltcp-desktop-windows-x86_64.exe"; do
    if [[ -f "$ROOT_DIR/dist/$name" ]]; then
        assets+=("$ROOT_DIR/dist/$name")
    fi
done

if [[ -f "$APK" ]]; then
    assets+=("$APK")
fi

if [[ ${#assets[@]} -eq 0 ]]; then
    echo "[release] No artifacts in dist/. Run ./scripts/build-desktop.sh and/or ./scripts/build-android.sh"
    exit 1
fi

echo "[release] Pushing main and tag $TAG..."
git push origin main
if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "[release] Tag $TAG already exists locally — not rewriting."
else
    git tag -a "$TAG" -m "SRLTCP $TAG"
fi
git push origin "$TAG"

echo "[release] Creating GitHub Release $TAG..."
gh release create "$TAG" \
    --repo "$REPO" \
    --title "SRLTCP $TAG" \
    --notes "## SRLTCP $TAG

Prebuilt desktop binaries and Android APK — no compiler required.

### Desktop
\`\`\`bash
git clone https://github.com/$REPO.git && cd SRLTCPv2
./run.sh          # Linux/macOS
run.bat           # Windows
\`\`\`

### Android
\`\`\`bash
adb install SRLTCPv2-${VERSION}.apk
\`\`\`

See [README.md](https://github.com/$REPO/blob/main/README.md) and [docs/BUILD.md](docs/BUILD.md)." \
    "${assets[@]}" 2>/dev/null || \
gh release upload "$TAG" "${assets[@]}" --repo "$REPO" --clobber

echo "[release] Done: https://github.com/$REPO/releases/tag/$TAG"