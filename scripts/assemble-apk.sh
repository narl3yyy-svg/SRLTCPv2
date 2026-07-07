#!/usr/bin/env bash
# Gradle-only APK build — requires jniLibs and UniFFI bindings already built.
exec "$(dirname "$0")/build-android.sh" --apk-only "$@"