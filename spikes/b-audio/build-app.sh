#!/usr/bin/env bash
# Builds and installs the spike B app and pushes the album.
# Usage: ./build-app.sh SERIAL [x86_64|arm64-v8a]
# Needs the Android Studio JBR (Java 21) and the VS-bundled cmake (opusic-sys).
set -euo pipefail
cd "$(dirname "$0")"
export MSYS_NO_PATHCONV=1   # keep Git Bash from rewriting /sdcard/... paths
S=$1
ABI=${2:-x86_64}
VS=$("/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe" -latest -products '*' -property installationPath)
CM="$(cygpath "$VS")/Common7/IDE/CommonExtensions/Microsoft/CMake"
export PATH="$CM/CMake/bin:$CM/Ninja:$PATH"
export CMAKE_GENERATOR=Ninja
export JAVA_HOME="${JAVA_HOME_JBR:-/c/Program Files/Android/Android Studio/jbr}"
export ANDROID_HOME="${ANDROID_HOME:-$LOCALAPPDATA/Android/Sdk}"
(cd android && ./gradlew --no-daemon -q :app:assembleRelease -PrustAbis="$ABI")
APK=android/app/build/outputs/apk/release/app-release.apk
adb -s "$S" install -r "$APK"
P=dev.ristle.spike.b
adb -s "$S" shell pm grant $P android.permission.POST_NOTIFICATIONS
# Straight into the app-owned files/ dir: a subdirectory made by adb belongs to shell.
adb -s "$S" shell am start -W -n $P/.MainActivity >/dev/null; sleep 2; adb -s "$S" shell am force-stop $P
DIR=/sdcard/Android/data/$P/files
for f in t0.mp3 t1.mp3 t2.mp3 t0.m4a t1.m4a t2.m4a; do
  adb -s "$S" push "media/gapless/$f" "$DIR/" >/dev/null 2>&1
done
ls -la "$APK"
