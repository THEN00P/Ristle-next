"""Measures GUI-thread ticks vs render-thread frames with and without
render-thread Animators (the half-rate finding)."""
import sys, collections
from spikelib import run, Script
import analyze

def rates(d):
    rg = analyze.ring(d)
    ev = rg["events"]
    c = collections.Counter(k for q, k, a, t in ev)
    dur = (ev[-1][0] - ev[0][0]) / 1e7
    return {"gui_ticks_per_s": round(c[1] / dur, 1), "syncs_per_s": round(c[2] / dur, 1),
            "frames_per_s": round(c[6] / dur, 1)}

if __name__ == "__main__":
    tag = sys.argv[1]
    for anim in (0, 1):
        d = run(f"animrate_{tag}_animators{anim}", {"SPIKE_ANIMATORS": anim}, None, mode="pm", tail=4)
        fs = analyze.frame_stats(analyze.presentmon(d), 0, 1 << 62, analyze.refresh_hz(analyze.meta(d)))
        print(tag, "animators", anim, rates(d), "presentmon frames", fs["frames"], "p50", fs["p50_ms"])
