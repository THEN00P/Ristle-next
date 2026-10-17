#!/usr/bin/env bash
# Background playback soak: start playback, go home, screen off, optionally
# force Doze, send transport commands through the media session while the
# screen is off, and sample CPU from /proc. Usage:
#   ./soak.sh SERIAL MINUTES OUT_DIR [--doze]
# Install first with ./build-app.sh SERIAL <abi>. On a phone, unplug USB power
# if you can (wireless adb) or pass --doze, which fakes unplugged + idle.
set -euo pipefail
cd "$(dirname "$0")"
export MSYS_NO_PATHCONV=1
S=$1; MIN=$2; OUT=$3; DOZE=${4:-}
A="adb -s $S"
P=dev.ristle.spike.b
mkdir -p "$OUT"
log() { echo "$(date +%H:%M:%S) $*" | tee -a "$OUT/events.txt"; }

if [ -z "${SOAK_ATTACH:-}" ]; then   # SOAK_ATTACH=1 measures playback already running
$A shell am force-stop $P
$A logcat -c
$A shell input keyevent KEYCODE_WAKEUP 2>/dev/null || true
$A shell wm dismiss-keyguard >/dev/null 2>&1 || true
# The app must start visible: autoplay presses Play once its controller connects.
until $A shell dumpsys power | grep -q "mWakefulness=Awake" && ! $A shell dumpsys window | grep -q "isKeyguardShowing=true"; do
  echo "Unlock the phone and leave the screen on."; sleep 5
done
$A shell am start -n $P/.MainActivity --ez autoplay true >/dev/null
for i in $(seq 1 30); do
  $A shell dumpsys media_session | grep -qE "state=(PLAYING|3)," && break
  sleep 2
done
$A shell dumpsys media_session | grep -qE "state=(PLAYING|3)," || { echo "playback did not start"; exit 1; }
echo "PLAYING: press Home, then the power button now."
sleep 10
fi
PID=$($A shell pidof $P | tr -d '\r')
ASPID=$($A shell pidof audioserver | tr -d '\r')
log "started pid=$PID audioserver=$ASPID device=$($A shell getprop ro.product.model | tr -d '\r')"
# MIUI refuses injected keys without "USB debugging (Security settings)":
# then press Home and the power button by hand. Wait for the screen to go off.
$A shell input keyevent KEYCODE_HOME 2>/dev/null || true
sleep 2
$A shell input keyevent KEYCODE_SLEEP 2>/dev/null || echo "Press Home, then the power button on the phone."
for i in $(seq 1 60); do
  $A shell dumpsys power | grep -q "mWakefulness=Awake" || break
  sleep 3
done
log "home + screen off ($($A shell dumpsys power | grep -m1 -o 'mWakefulness=[A-Za-z]*'))"
if [ "$DOZE" = "--doze" ]; then
  # Undo the fake unplug and forced idle however the script ends.
  trap '$A shell dumpsys deviceidle unforce >/dev/null 2>&1; $A shell dumpsys battery reset >/dev/null 2>&1' EXIT
  $A shell dumpsys battery unplug
  $A shell dumpsys deviceidle force-idle | tr -d '\r' | tee -a "$OUT/events.txt"
fi

TICKS=$($A shell getconf CLK_TCK | tr -d '\r')
cpu() { # utime+stime ticks of a pid
  $A shell cat /proc/$1/stat | tr -d '\r' | sed 's/.*) //' | awk '{print $12+$13}'
}
threads() { # name=ticks for every thread of the app
  $A shell "for t in /proc/$PID/task/*; do echo \$(cat \$t/comm)=\$(cut -d' ' -f14,15 \$t/stat); done" | tr -d '\r' | awk -F= '{split($2,a," "); print $1"="a[1]+a[2]}' | sort | tr '\n' ' '
}
c0=$(cpu $PID); a0=$(cpu $ASPID); t0=$(date +%s)
echo "# minute app_cpu_pct audioserver_cpu_pct (share of one core, interval) | threads (ticks, CLK_TCK=$TICKS)" > "$OUT/cpu.txt"
pc=$c0; pa=$a0; pt=$t0
for m in $(seq 1 "$MIN"); do
  sleep 60
  # SOAK_NO_DISPATCH=1: skip the shell media-button commands. On Android 11,
  # `cmd media_session dispatch` has an empty caller package and Media3 1.8.0
  # crashes on it in onMediaButtonEvent (Mi 9T, spike B).
  [ -n "${SOAK_NO_DISPATCH:-}" ] && [ "$m" != 999 ] && m_cmd=0 || m_cmd=$m
  case $m_cmd in
    10) log "dispatch pause (screen off)"; $A shell cmd media_session dispatch pause; sleep 5
        log "dispatch play"; $A shell cmd media_session dispatch play ;;
    20) log "dispatch next"; $A shell cmd media_session dispatch next ;;
  esac
  # Check liveness before reading /proc/$PID: under set -e a vanished process
  # would end the script before the log is collected.
  # A restarted app has a new PID (the Mi 9T's crash restarted it), so compare.
  now_pid=$($A shell pidof $P | tr -d '\r' || true)
  if [ "$now_pid" != "$PID" ]; then log "APP DIED at minute $m (pid now '${now_pid}')"; break; fi
  now=$(date +%s); c=$(cpu $PID); a=$(cpu $ASPID)
  dt=$((now - pt))
  echo "$m $(awk -v d=$((c - pc)) -v t=$dt -v k=$TICKS 'BEGIN{printf "%.2f", d/k/t*100}') $(awk -v d=$((a - pa)) -v t=$dt -v k=$TICKS 'BEGIN{printf "%.2f", d/k/t*100}') | $(threads)" >> "$OUT/cpu.txt"
  pc=$c; pa=$a; pt=$now
done
now=$(date +%s)
if [ "$($A shell pidof $P | tr -d '\r' || true)" = "$PID" ]; then
log "total app CPU $(awk -v d=$(( $(cpu $PID) - c0 )) -v t=$((now - t0)) -v k=$TICKS 'BEGIN{printf "%.3f", d/k/t*100}')% of one core over $((now - t0)) s, audioserver $(awk -v d=$(( $(cpu $ASPID) - a0 )) -v t=$((now - t0)) -v k=$TICKS 'BEGIN{printf "%.3f", d/k/t*100}')%"
fi
$A shell dumpsys power | grep -E "mWakefulness=|mDeviceIdleMode|Display Power" | tr -d '\r' >> "$OUT/events.txt" || true
$A shell dumpsys activity services $P | grep -E "isForeground" | tr -d '\r' >> "$OUT/events.txt" || true
$A logcat -d -s SPIKE_B:* ristle-audio:* AndroidRuntime:E AudioTrack:W AAudio:W > "$OUT/logcat.txt"
$A shell input keyevent KEYCODE_WAKEUP 2>/dev/null || true
grep SPIKE_B_HEALTH "$OUT/logcat.txt" | tail -3
tail -4 "$OUT/events.txt"
