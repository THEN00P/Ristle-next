"""Builds the HLS variants ffmpeg cannot write. Run with python -I.
packed-adts DIR SRC.m4a : ADTS segments, each prefixed with the ID3 PRIV
    transportStreamTimestamp tag Apple's packed-audio spec requires.
encrypt DIR KEYFILE     : AES-128-CBC each fMP4 segment (IV = media sequence),
    via openssl, and add EXT-X-KEY to the playlist.
"""
import os, re, subprocess, sys

def id3_priv_timestamp(pts90k):
    owner = b"com.apple.streaming.transportStreamTimestamp\x00"
    data = owner + (pts90k & ((1 << 33) - 1)).to_bytes(8, "big")
    frame = b"PRIV" + len(data).to_bytes(4, "big") + b"\x00\x00" + data
    size = len(frame)
    syncsafe = bytes([(size >> 21) & 0x7F, (size >> 14) & 0x7F, (size >> 7) & 0x7F, size & 0x7F])
    return b"ID3\x04\x00\x00" + syncsafe + frame

def packed_adts(d, src):
    os.makedirs(d, exist_ok=True)
    subprocess.check_call(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", src, "-c:a", "copy",
        "-f", "segment", "-segment_time", "6", "-segment_format", "adts",
        "-segment_list", os.path.join(d, "index.m3u8"), "-segment_list_type", "m3u8",
        os.path.join(d, "seg%03d.aac")])
    pl = open(os.path.join(d, "index.m3u8")).read()
    t = 0.0
    for line in pl.splitlines():
        m = re.match(r"#EXTINF:([\d.]+)", line)
        if m:
            dur = float(m.group(1))
        elif line.endswith(".aac"):
            p = os.path.join(d, line)
            body = open(p, "rb").read()
            open(p, "wb").write(id3_priv_timestamp(int(t * 90000)) + body)
            t += dur

def encrypt(d, keyfile):
    key = open(keyfile, "rb").read().hex()
    pl = open(os.path.join(d, "index.m3u8")).read().splitlines()
    out, seq = [], 0
    for line in pl:
        if line.startswith("#EXT-X-MEDIA-SEQUENCE:"):
            seq = int(line.split(":")[1])
        if line.startswith("#EXT-X-MAP"):
            # After the map: a key before EXT-X-MAP would also cover init.mp4.
            out.extend([line, '#EXT-X-KEY:METHOD=AES-128,URI="key.bin"'])
            continue
        if line.endswith(".m4s"):
            p = os.path.join(d, line)
            iv = seq.to_bytes(16, "big").hex()
            subprocess.check_call(["openssl", "enc", "-aes-128-cbc", "-K", key, "-iv", iv, "-in", p, "-out", p + ".enc"])
            os.replace(p + ".enc", p)
            seq += 1
        out.append(line)
    open(os.path.join(d, "index.m3u8"), "w", newline="\n").write("\n".join(out) + "\n")

if __name__ == "__main__":
    {"packed-adts": packed_adts, "encrypt": encrypt}[sys.argv[1]](*sys.argv[2:])
