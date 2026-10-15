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

$A shell am force-stop $P
$A logcat -c
$A shell input keyevent KEYCODE_WAKEUP
$A shell wm dismiss-keyguard >/dev/null 2>&1 || true
$A shell am start -n $P/.MainActivity --ez autoplay true >/dev/null
sleep 15
PID=$($A shell pidof $P | tr -d '\r')
ASPID=$($A shell pidof audioserver | tr -d '\r')
log "started pid=$PID audioserver=$ASPID device=$($A shell getprop ro.product.model | tr -d '\r')"
$A shell input keyevent KEYCODE_HOME
sleep 2
$A shell input keyevent KEYCODE_SLEEP
log "home + screen off"
if [ "$DOZE" = "--doze" ]; then
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
  case $m in
    10) log "dispatch pause (screen off)"; $A shell cmd media_session dispatch pause; sleep 5
        log "dispatch play"; $A shell cmd media_session dispatch play ;;
    20) log "dispatch next"; $A shell cmd media_session dispatch next ;;
  esac
  now=$(date +%s); c=$(cpu $PID); a=$(cpu $ASPID)
  dt=$((now - pt))
  echo "$m $(awk -v d=$((c - pc)) -v t=$dt -v k=$TICKS 'BEGIN{printf "%.2f", d/k/t*100}') $(awk -v d=$((a - pa)) -v t=$dt -v k=$TICKS 'BEGIN{printf "%.2f", d/k/t*100}') | $(threads)" >> "$OUT/cpu.txt"
  pc=$c; pa=$a; pt=$now
  if ! $A shell pidof $P >/dev/null; then log "APP DIED"; break; fi
done
now=$(date +%s)
log "total app CPU $(awk -v d=$(( $(cpu $PID) - c0 )) -v t=$((now - t0)) -v k=$TICKS 'BEGIN{printf "%.3f", d/k/t*100}')% of one core over $((now - t0)) s, audioserver $(awk -v d=$(( $(cpu $ASPID) - a0 )) -v t=$((now - t0)) -v k=$TICKS 'BEGIN{printf "%.3f", d/k/t*100}')%"
$A shell dumpsys power | grep -E "mWakefulness=|mDeviceIdleMode|Display Power" | tr -d '\r' >> "$OUT/events.txt" || true
$A shell dumpsys activity services $P | grep -E "isForeground" | tr -d '\r' >> "$OUT/events.txt" || true
if [ "$DOZE" = "--doze" ]; then $A shell dumpsys deviceidle unforce; $A shell dumpsys battery reset; fi
$A logcat -d -s SPIKE_B:* ristle-audio:* AndroidRuntime:E AudioTrack:W AAudio:W > "$OUT/logcat.txt"
$A shell input keyevent KEYCODE_WAKEUP
grep SPIKE_B_HEALTH "$OUT/logcat.txt" | tail -3
tail -4 "$OUT/events.txt"
