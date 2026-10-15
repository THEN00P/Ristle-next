#!/usr/bin/env bash
# Cross-compiles the audiolab CLI for Android (x86_64 emulator, arm64 phone).
set -euo pipefail
cd "$(dirname "$0")"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$LOCALAPPDATA/Android/Sdk/ndk/27.1.12297006}"
VS=$("/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe" -latest -products '*' -property installationPath)
CM="$(cygpath "$VS")/Common7/IDE/CommonExtensions/Microsoft/CMake"
export PATH="$CM/CMake/bin:$CM/Ninja:$PATH"
STUB="$(cygpath -m "$PWD/android-stubs")"
export CMAKE_GENERATOR=Ninja
# fdk-aac-sys compiles its .cpp files through cc without .cpp(true), so CFLAGS apply.
env "CFLAGS_x86_64-linux-android=-I$STUB" "CFLAGS_aarch64-linux-android=-I$STUB" cargo ndk -t x86_64 -t arm64-v8a --platform 26 build --release -p audiolab "$@"
