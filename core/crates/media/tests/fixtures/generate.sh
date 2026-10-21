#!/usr/bin/env bash
# Generates the ristle-media test fixtures from a synthetic signal, so their
# licence is ours (docs/plan/phase-2.md, "Tests"). No third-party audio.
#
# Needs ffmpeg with libmp3lame, libvorbis and libopus. The outputs are
# committed; rerun this only to change them, and keep every file under 200 KB.
#
# The signal is deterministic (no noise), and `tests/common/signal.rs`
# computes the same expression, so tests compare decoded audio with it
# directly. Keep the two in sync.
set -euo pipefail
cd "$(dirname "$0")"
FF="ffmpeg -hide_banner -loglevel error -y -bitexact"
META="-map_metadata -1 -fflags +bitexact -flags:a +bitexact"

# Left: a 440 Hz tone with 3 Hz tremolo plus a rising chirp. Right: 554.37 Hz
# with 2 Hz tremolo plus a falling chirp. The chirps make every position
# unique, so a cross-correlation finds the alignment without ambiguity.
L='0.25*sin(2*PI*440*t)*(0.75+0.25*sin(2*PI*3*t))+0.15*sin(2*PI*(150*t+300*t*t))'
R='0.25*sin(2*PI*554.37*t)*(0.75+0.25*cos(2*PI*2*t))+0.15*sin(2*PI*(2000*t-250*t*t))'
source() { # rate frames out.wav
  $FF -f lavfi -i "aevalsrc=exprs='$L|$R':s=$1:d=10" -af "atrim=end_sample=$2" \
    -c:a pcm_s16le $META "$3"
}
cut() { # source.wav start end out.wav
  $FF -i "$1" -af "atrim=start_sample=$2:end_sample=$3,asetpts=N/SR/TB" -c:a pcm_s16le $META "$4"
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# --- Gapless album: three tracks cut from one signal at points that are not
# multiples of any codec's frame size. Track lengths are in tests/common.
source 44100 158760 "$tmp/s44.wav"
CUTS=(0 52517 105011 158760)
for i in 0 1 2; do
  cut "$tmp/s44.wav" "${CUTS[$i]}" "${CUTS[$((i + 1))]}" "$tmp/t$i.wav"
  $FF -i "$tmp/t$i.wav" -c:a libmp3lame -b:a 128k $META "album-$i.mp3"
  $FF -i "$tmp/t$i.wav" -c:a aac -b:a 128k $META "album-$i.m4a"
  $FF -i "$tmp/t$i.wav" -c:a libvorbis -q:a 3 $META "album-$i.ogg"
  $FF -i "$tmp/t$i.wav" -c:a flac $META "album-$i.flac"
done
# Opus always decodes at 48 kHz, so it has its own source and cuts.
source 48000 172800 "$tmp/s48.wav"
CUTS48=(0 57131 114287 172800)
for i in 0 1 2; do
  cut "$tmp/s48.wav" "${CUTS48[$i]}" "${CUTS48[$((i + 1))]}" "$tmp/u$i.wav"
  $FF -i "$tmp/u$i.wav" -c:a libopus -b:a 96k $META "album-$i.opus"
done

# --- One file per remaining container, all cut like album track 1.
$FF -i "$tmp/t1.wav" -af atrim=end_sample=44100 -c:a pcm_s16le $META track.wav
$FF -i "$tmp/t1.wav" -c:a aac -b:a 128k -f adts $META track.aac
$FF -i "$tmp/t1.wav" -c:a libvorbis -q:a 3 $META track-vorbis.webm
$FF -i "$tmp/u1.wav" -c:a libopus -b:a 96k $META track-opus.webm
$FF -i "$tmp/t1.wav" -c:a flac $META track.mka
# MPEG-TS as HLS live radio serves it: ADTS AAC, and MPEG audio.
$FF -i "$tmp/t1.wav" -c:a aac -b:a 128k -f mpegts $META track-aac.ts
$FF -i "$tmp/t1.wav" -c:a libmp3lame -b:a 128k -f mpegts $META track-mp3.ts

# --- A muxed MP4 whose first track is video with its own edit list (B-frames
# delay the video), and whose audio is album track 1. The host must read the
# edit list of the audio track it plays, never the first one it finds.
$FF -f lavfi -i "testsrc=size=16x16:rate=10:duration=1.2" -i "$tmp/t1.wav" \
  -map 0:v -map 1:a -c:v mpeg4 -bf 2 -q:v 10 -video_track_timescale 1000 -c:a aac -b:a 128k $META muxed.mp4

# --- HLS renditions of the whole 44.1 kHz album signal, 1 s segments. Tests
# derive the encrypted, byte-range and packed-audio variants from these.
HLS="-hls_time 1 -hls_playlist_type vod -hls_flags independent_segments"
rm -rf hls && mkdir -p hls/fmp4 hls/ts hls/muxed
$FF -i "$tmp/s44.wav" -c:a aac -b:a 128k $META -f hls $HLS -hls_segment_type fmp4   -hls_fmp4_init_filename init.mp4 -hls_segment_filename hls/fmp4/seg%d.m4s hls/fmp4/index.m3u8
$FF -i "$tmp/s44.wav" -c:a aac -b:a 128k $META -f hls $HLS -hls_segment_type mpegts   -hls_segment_filename hls/ts/seg%d.ts hls/ts/index.m3u8
# Video and audio in one rendition: each moof carries a video traf too.
$FF -f lavfi -i "testsrc=size=16x16:rate=10:duration=3.6" -i "$tmp/s44.wav" -map 0:v -map 1:a   -c:v mpeg4 -g 10 -q:v 10 -c:a aac -b:a 128k $META -f hls $HLS -hls_segment_type fmp4   -hls_fmp4_init_filename init.mp4 -hls_segment_filename hls/muxed/seg%d.m4s hls/muxed/index.m3u8
ls -l hls/*

ls -l ./*.mp3 ./*.m4a ./*.ogg ./*.flac ./*.opus ./*.wav ./*.aac ./*.webm ./*.mka ./*.ts ./*.mp4
