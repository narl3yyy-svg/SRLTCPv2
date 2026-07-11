#!/usr/bin/env bash
# Shared Android build environment detection — source from other scripts.
# Usage: source "$(dirname "$0")/lib/android-env.sh"

find_jdk17() {
    local candidates=(
        "${JAVA_HOME:-}"
        "/usr/lib/jvm/java-17-openjdk"
        "/usr/lib/jvm/java-17-openjdk-amd64"
        "/usr/lib/jvm/temurin-17-jdk"
        "/usr/lib/jvm/jdk-17"
        "$HOME/.jdks/temurin-17"
        "$HOME/.sdkman/candidates/java/17.0.13-tem"
        "/opt/homebrew/opt/openjdk@17"
        "/usr/local/opt/openjdk@17"
    )
    for jdk in "${candidates[@]}"; do
        [[ -z "$jdk" || ! -d "$jdk" ]] && continue
        if "$jdk/bin/java" -version 2>&1 | grep -qE 'version "17'; then
            export JAVA_HOME="$jdk"
            return 0
        fi
    done
    return 1
}

find_android_sdk() {
    local candidates=(
        "${ANDROID_HOME:-}"
        "${ANDROID_SDK_ROOT:-}"
        "$HOME/Android/Sdk"
        "$HOME/Library/Android/sdk"
    )
    for sdk in "${candidates[@]}"; do
        if [[ -d "$sdk" && -d "$sdk/platform-tools" ]]; then
            export ANDROID_HOME="$sdk"
            export ANDROID_SDK_ROOT="$sdk"
            return 0
        fi
    done
    return 1
}

find_android_ndk() {
    find_android_sdk || return 1
    if [[ -n "${ANDROID_NDK_HOME:-}" && -d "$ANDROID_NDK_HOME" ]]; then
        return 0
    fi
    local ndk_root="$ANDROID_HOME/ndk"
    if [[ ! -d "$ndk_root" ]]; then
        return 1
    fi
    # Pick the newest NDK version directory
    local latest
    latest=$(ls -1 "$ndk_root" 2>/dev/null | sort -V | tail -1)
    if [[ -n "$latest" && -d "$ndk_root/$latest" ]]; then
        export ANDROID_NDK_HOME="$ndk_root/$latest"
        return 0
    fi
    return 1
}

write_local_properties() {
    local android_dir="$1"
    find_android_sdk || {
        echo "[android] ERROR: Android SDK not found. Set ANDROID_HOME or install Android SDK." >&2
        return 1
    }
    echo "sdk.dir=$ANDROID_HOME" > "$android_dir/local.properties"
}

verify_jni_libs() {
    local jni_dir="$1"
    # Slim default is arm64-only; universal builds may include more ABIs.
    if [[ "${SRLTCP_UNIVERSAL_APK:-0}" == "1" ]]; then
        local abis=("arm64-v8a" "armeabi-v7a" "x86_64")
    else
        local abis=("arm64-v8a")
    fi
    local missing=()
    for abi in "${abis[@]}"; do
        if [[ ! -f "$jni_dir/$abi/libsrltcp_core.so" ]]; then
            missing+=("$abi")
        fi
    done
    if [[ ${#missing[@]} -gt 0 ]]; then
        echo "[android] Missing native libs for: ${missing[*]}" >&2
        return 1
    fi
    return 0
}