#!/usr/bin/env bash
# 10-minute soak on a running app: rotation churn, background, foreground again.
# Usage: scripts/soak.sh <out-dir>. Needs ANDROID_SERIAL when several devices are attached.
set -u
OUT=${1:-soak-out}; mkdir -p "$OUT"
PKG=dev.ristle.spike.d
mem() { adb shell dumpsys meminfo $PKG | grep -E "TOTAL PSS|Java Heap|Native Heap|Activities|AppContexts|Views:" > "$OUT/meminfo-$1.txt"; }
rotate_for() { # seconds
  local end=$((SECONDS + $1)) r=0
  while [ $SECONDS -lt $end ]; do r=$((1 - r)); adb shell settings put system user_rotation $r; sleep 5; done
  adb shell settings put system user_rotation 0
}
adb shell settings put system accelerometer_rotation 0
adb logcat -c
mem start
echo "$(date +%T) phase A: rotation churn 240 s"; rotate_for 240
mem after-rotation
echo "$(date +%T) phase B: background 240 s"; adb shell input keyevent KEYCODE_HOME; sleep 240
mem background
echo "$(date +%T) phase C: foreground + churn 120 s"; adb shell am start -n $PKG/.MainActivity > /dev/null; rotate_for 120
sleep 10; adb shell input keyevent KEYCODE_HOME; sleep 5; adb shell am start -n $PKG/.MainActivity > /dev/null; sleep 15
mem end
adb logcat -d > "$OUT/logcat.txt"
echo "$(date +%T) done"
