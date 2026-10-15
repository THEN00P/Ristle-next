#!/usr/bin/env bash
# Generates all test media from a synthetic signal (no third-party audio).
# Needs ffmpeg with libmp3lame, libvorbis, libopus; HE-AAC comes from
# `audiolab fdkenc` (FDK), because ffmpeg's native AAC encoder has no SBR.
set -euo pipefail
cd "$(dirname "$0")"
OUT=media
LAB=target/release/audiolab
mkdir -p $OUT/bench $OUT/gapless
FF="ffmpeg -hide_banner -loglevel error -y"

# Chord with vibrato and tremolo, a kick every 0.5 s, noise hi-hats, a chirp.
sig() { # rate seconds
  local L="0.12*sin(2*PI*220*pow(2,floor(mod(t,8)/2)*2/12)*t)*(1+0.3*sin(2*PI*3*t))+0.08*sin(2*PI*277.18*t+2*sin(2*PI*5*t))+0.06*sin(2*PI*329.63*t)+0.3*exp(-25*mod(t,0.5))*sin(2*PI*(55+150*exp(-30*mod(t,0.5)))*t)+0.1*exp(-40*mod(t+0.25,0.5))*(2*random(0)-1)+0.04*sin(2*PI*(100*t+150*t*t/$2))"
  local R="0.12*sin(2*PI*330*pow(2,floor(mod(t,8)/2)*2/12)*t)*(1+0.3*sin(2*PI*2.5*t))+0.08*sin(2*PI*246.94*t+2*sin(2*PI*4*t))+0.06*sin(2*PI*392*t)+0.3*exp(-25*mod(t,0.5))*sin(2*PI*(55+150*exp(-30*mod(t,0.5)))*t)+0.1*exp(-40*mod(t+0.125,0.5))*(2*random(1)-1)+0.04*sin(2*PI*(9000*t-150*t*t/$2))"
  echo "aevalsrc=exprs='$L|$R':s=$1:d=$2[a];anoisesrc=c=pink:r=$1:a=0.04:d=$2,aformat=channel_layouts=mono,pan=stereo|c0=c0|c1=c0[n];[a][n]amix=inputs=2:normalize=0"
}

# --- Decode CPU set: 60 s ---
$FF -filter_complex "$(sig 44100 60)" -c:a pcm_s16le $OUT/bench/src44.wav
$FF -filter_complex "$(sig 48000 60)" -c:a pcm_s16le $OUT/bench/src48.wav
B=$OUT/bench
$FF -i $B/src44.wav -c:a libmp3lame -b:a 320k $B/mp3_320.mp3
$FF -i $B/src44.wav -c:a aac -b:a 256k $B/aaclc_256.m4a
$FF -i $B/src44.wav -c:a libvorbis -b:a 320k $B/vorbis_320.ogg
$FF -i $B/src48.wav -c:a libopus -b:a 160k $B/opus_160.opus
$FF -i $B/src48.wav -c:a libopus -b:a 160k $B/opus_160.webm
$FF -i $B/src44.wav -c:a flac $B/flac_16_44.flac
$FF -filter_complex "$(sig 96000 60)" -c:a flac -sample_fmt s32 -bits_per_raw_sample 24 $B/flac_24_96.flac
$FF -i $B/src44.wav -c:a aac -b:a 256k -f adts $B/aaclc_256.aac
$FF -i $B/src44.wav -f s16le $B/src44.s16
$LAB fdkenc $B/src44.s16 $B/heaac_v1.aac 44100 he 3
$LAB fdkenc $B/src44.s16 $B/heaac_v2.aac 44100 hev2 2
$LAB fdkenc $B/src44.s16 $B/aaclc_fdk.aac 44100 lc 5
$FF -i $B/heaac_v1.aac -c:a copy $B/heaac_v1.m4a
rm $B/src44.s16

# --- Gapless set: 3 tracks cut from one 30 s signal at non-frame-aligned points ---
G=$OUT/gapless
$FF -filter_complex "$(sig 44100 30)" -c:a pcm_s16le $G/source.wav
CUTS=(0 437731 891113 1323000)   # 9.93 s, 10.29 s, 9.80 s
for i in 0 1 2; do
  s=${CUTS[$i]}; e=${CUTS[$((i+1))]}
  $FF -i $G/source.wav -af "atrim=start_sample=$s:end_sample=$e,asetpts=N/SR/TB" -c:a pcm_s16le $G/t$i.wav
  $FF -i $G/t$i.wav -c:a libmp3lame -b:a 320k $G/t$i.mp3
  $FF -i $G/t$i.wav -c:a aac -b:a 256k $G/t$i.m4a
  $FF -i $G/t$i.wav -c:a aac -b:a 256k -f adts $G/t$i.aac
  $FF -i $G/t$i.wav -c:a libvorbis -b:a 320k $G/t$i.ogg
  $FF -i $G/t$i.wav -c:a libopus -b:a 160k $G/t$i.opus
  $FF -i $G/t$i.wav -c:a flac $G/t$i.flac
done
# Opus always decodes at 48 kHz, so it gets its own 48 kHz source and cuts.
$FF -filter_complex "$(sig 48000 30)" -c:a pcm_s16le $G/source48.wav
CUTS48=(0 476442 969917 1440000)
for i in 0 1 2; do
  s=${CUTS48[$i]}; e=${CUTS48[$((i+1))]}
  $FF -i $G/source48.wav -af "atrim=start_sample=$s:end_sample=$e,asetpts=N/SR/TB" -c:a pcm_s16le $G/u$i.wav
  $FF -i $G/u$i.wav -c:a libopus -b:a 160k $G/u$i.opus
done
ls -la $B $G

# --- HLS set: 4 min AAC-LC 160 kbps (SoundCloud's AAC tier), 6 s segments ---
H=$OUT/hls
mkdir -p $H/fmp4 $H/adts $H/ts $H/fmp4-aes $H/ts-aes
$FF -filter_complex "$(sig 44100 240)" -c:a pcm_s16le $H/source.wav
$FF -i $H/source.wav -c:a aac -b:a 160k $H/source.m4a
HLS="-c:a copy -f hls -hls_time 6 -hls_playlist_type vod"
$FF -i $H/source.m4a $HLS -hls_segment_type fmp4 -hls_fmp4_init_filename init.mp4 -hls_segment_filename $H/fmp4/seg%03d.m4s $H/fmp4/index.m3u8
python -I tools-hls.py packed-adts $H/adts $H/source.m4a
$FF -i $H/source.m4a $HLS -hls_segment_type mpegts -hls_segment_filename $H/ts/seg%03d.ts $H/ts/index.m3u8
# AES-128: fixed test key, IV left to the media sequence number.
printf '0123456789abcdef' > $H/key.bin
for d in fmp4-aes ts-aes; do printf 'key.bin\n%s\n' "$(cygpath -m "$PWD/$H/key.bin")" > $H/$d/keyinfo; cp $H/key.bin $H/$d/; done
# ffmpeg cannot write encrypted fMP4, so encrypt a plain copy.
cp $H/fmp4/* $H/fmp4-aes/ && python -I tools-hls.py encrypt $H/fmp4-aes $H/key.bin
$FF -i $H/source.m4a $HLS -hls_segment_type mpegts -hls_key_info_file $H/ts-aes/keyinfo -hls_segment_filename $H/ts-aes/seg%03d.ts $H/ts-aes/index.m3u8
