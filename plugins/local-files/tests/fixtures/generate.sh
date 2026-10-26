#!/usr/bin/env bash
# Generates the Local Files Plugin's test fixtures: tagged files from a
# synthetic tone, so their licence is ours (docs/plan/phase-2.md, "Tests").
#
# Needs ffmpeg with libmp3lame, libvorbis and libopus. The outputs are
# committed; rerun this only to change them, and keep every file under
# 200 KB. tests/probe.rs asserts the tags written here.
set -euo pipefail
cd "$(dirname "$0")"
FF="ffmpeg -hide_banner -loglevel error -y -bitexact -fflags +bitexact -flags:a +bitexact"
tone() { # seconds channels
  echo "-f lavfi -i sine=frequency=440:sample_rate=44100:duration=$1 -ac $2"
}
TAGS=(
  -metadata title="First Light"
  -metadata artist="Ada Example"
  -metadata album_artist="The Examples"
  -metadata album="Synthetic Mornings"
  -metadata track="3/12"
  -metadata disc="2/2"
  -metadata date="2019-03-14"
  -metadata genre="Ambient"
  -metadata ISRC="GB-AYE-06-01498"
)

# One short tagged file per format the host plays. ffmpeg's MP4 muxer
# writes no ISRC, so only the MP4s lack one.
$FF $(tone 1 2) "${TAGS[@]}" -c:a flac tagged.flac
# ffmpeg writes unknown keys as TXXX; taggers use the TSRC frame.
$FF $(tone 1 2) "${TAGS[@]}" -metadata ISRC= -metadata TSRC="GBAYE0601498"   -c:a libmp3lame -b:a 128k -id3v2_version 4 tagged.mp3
$FF $(tone 1 2) "${TAGS[@]}" -c:a aac -b:a 128k tagged.m4a
$FF $(tone 1 2) "${TAGS[@]}" -c:a alac tagged-alac.m4a
$FF $(tone 1 2) "${TAGS[@]}" -c:a libvorbis -q:a 3 tagged.ogg
$FF $(tone 1 2) "${TAGS[@]}" -ar 48000 -c:a libopus -b:a 64k tagged.opus
$FF $(tone 0.5 1) -metadata title="Plain Wave" -c:a pcm_s16le tagged.wav

# No tags at all: the title falls back to the file name.
$FF $(tone 1 1) -map_metadata -1 -c:a libmp3lame -b:a 64k untagged.mp3

# Year-only date and a track number without a total.
$FF $(tone 1 1) -metadata title="Year Only" -metadata date="1997" -metadata track="7" \
  -c:a flac year-only.flac

# Long files whose audio the probe must not read: tags at the front of an
# MP3, and an MP4 whose `moov` box sits after the audio (ffmpeg's default).
$FF $(tone 40 1) -metadata title="Long Walk" -c:a libmp3lame -b:a 32k long.mp3
$FF $(tone 40 1) -metadata title="Long Walk" -c:a aac -b:a 32k long.m4a

# Not audio, with an audio extension.
printf 'This is not audio.\n%.0s' $(seq 1 50) > fake.mp3
