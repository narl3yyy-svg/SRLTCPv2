#!/usr/bin/env bash
# Read workspace version from Cargo.toml
get_workspace_version() {
    local root="${1:-}"
    if [[ -z "$root" ]]; then
        root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
    fi
    grep '^version' "$root/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/'
}