#!/usr/bin/env bash
# Runs the decode-CPU benchmark and the gapless check on an Android device
# through adb (no app needed). Usage: ./android-bench.sh SERIAL OUT_PREFIX
#   SERIAL      emulator-5554, or the phone's serial from `adb devices`
#   OUT_PREFIX  results/<name>, writes <name>-bench.jsonl and <name>-gapless.txt
# Run ./gen-media.sh and ./build-android.sh first.
set -euo pipefail
cd "$(dirname "$0")"
export MSYS_NO_PATHCONV=1   # keep Git Bash from rewriting /data/... paths
S=$1; OUT=$2
A="adb -s $S"
ABI=$($A shell getprop ro.product.cpu.abi | tr -d '\r')
case $ABI in arm64-v8a) T=aarch64-linux-android ;; x86_64) T=x86_64-linux-android ;; *) echo "abi $ABI"; exit 1 ;; esac
D=/data/local/tmp/b
$A shell mkdir -p $D/bench $D/gapless
$A push target/$T/release/audiolab $D/ >/dev/null 2>&1
$A shell chmod 755 $D/audiolab
for f in mp3_320.mp3 aaclc_256.m4a aaclc_256.aac vorbis_320.ogg opus_160.opus opus_160.webm flac_16_44.flac flac_24_96.flac heaac_v1.aac heaac_v1.m4a heaac_v2.aac; do
  $A push media/bench/$f $D/bench/ >/dev/null 2>&1
done
$A push media/gapless/. $D/gapless/ >/dev/null 2>&1
{
  echo "# $($A shell getprop ro.product.manufacturer | tr -d '\r') $($A shell getprop ro.product.model | tr -d '\r'), $($A shell getprop ro.soc.model | tr -d '\r'), Android $($A shell getprop ro.build.version.release | tr -d '\r'), $ABI"
  echo "# cores: $($A shell 'cat /sys/devices/system/cpu/cpu*/cpufreq/cpuinfo_max_freq 2>/dev/null' | tr -d '\r' | tr '\n' ' ')"
  echo "# battery: $($A shell dumpsys battery | grep -E 'level|AC powered|USB powered' | tr -d '\r' | tr '\n' ' ')"
} > $OUT-bench.jsonl
FILES="bench/mp3_320.mp3 bench/aaclc_256.m4a bench/aaclc_256.aac bench/vorbis_320.ogg bench/opus_160.opus bench/opus_160.webm bench/flac_16_44.flac bench/flac_24_96.flac bench/heaac_v1.aac bench/heaac_v1.m4a bench/heaac_v2.aac"
# Warm-up pass (discarded): first run after push/boot pays page-cache and frequency ramp.
$A shell "cd $D && ./audiolab bench --min-cpu-ms 300 $FILES" >/dev/null
# Measured: unpinned (scheduler's choice), then pinned to the little cores (CPUs 0-3), the
# realistic home of a background audio thread on a big.LITTLE phone. taskset may be missing.
$A shell "cd $D && ./audiolab bench --resample --min-cpu-ms 3000 $FILES" | sed 's/^{/{"pin":"none",/' | tee -a $OUT-bench.jsonl
$A shell "cd $D && taskset 0f ./audiolab bench --min-cpu-ms 3000 $FILES" 2>/dev/null | sed 's/^{/{"pin":"cpu0-3",/' | tee -a $OUT-bench.jsonl || true
$A shell "cd $D && ./audiolab bench --fdk-all --min-cpu-ms 3000 bench/aaclc_256.m4a" | sed 's/^{/{"pin":"none",/' | tee -a $OUT-bench.jsonl
$A shell "cd $D && ./audiolab gapless gapless" | tee $OUT-gapless.txt
