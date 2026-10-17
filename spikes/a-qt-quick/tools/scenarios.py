"""Spike A scenarios and load ramps. Every run's summary is appended to
results/summary.jsonl; raw files stay in runs/<name>/."""
from __future__ import annotations

import json, sys, time
import analyze
from spikelib import ROOT, Script, run, TITLE, display

SUMMARY = ROOT / "results" / "summary.jsonl"

# Logical client coordinates (window is 1280 x 800 logical, client origin at
# 80, 60 logical; Main.qml).
ORIGIN = (80, 60)
LIST_X, LIST_TOP, LIST_BOTTOM = 600, 250, 690
QUEUE_X = 1100
MEASURE_FROM_MS = 900  # scripted input before this is warm-up


def scroll_script(kind, speed, secs=8.0, k=None, title=TITLE):
    """kind: fling (touch drag then release, `speed` px/s), wheel (`speed`
    notches/s), touchpad (`speed` px-ish/s as 120 Hz small wheel deltas)."""
    k = k or int(display()["scale"]) / 100
    s = Script()
    s.add(0, "focus", title)
    t0 = MEASURE_FROM_MS
    P = lambda v: round(v * k)        # y offsets and distances
    X = lambda v: round((ORIGIN[0] + v) * k)
    Y = lambda v: round((ORIGIN[1] + v) * k)
    # Warm-up touch before the measured window (MEASURE_FROM_MS): the first
    # touch after launch has a one-time cost, reported separately.
    s.touch_drag(100, X(LIST_X), Y(LIST_BOTTOM - 20), Y(LIST_BOTTOM - 40), 60)
    if kind == "fling":
        dist = 400
        dur = max(25, 1000 * dist / speed)
        t, up = t0, True
        while t < t0 + secs * 1000:
            y0, y1 = (LIST_BOTTOM, LIST_BOTTOM - dist) if up else (LIST_TOP, LIST_TOP + dist)
            s.touch_drag(t, X(LIST_X), Y(y0), Y(y1), dur)
            t += 1000
            up = not up if t > t0 + secs * 500 else up  # second half flings back
    elif kind == "wheel":
        s.add(t0, "move", X(LIST_X), Y(450))
        s.wheel_stream(t0 + 50, secs * 1000, -120, speed)
    elif kind == "touchpad":
        s.add(t0, "move", X(LIST_X), Y(450))
        s.wheel_stream(t0 + 50, secs * 1000, -max(1, round(speed / 120 / 2.4)), 120)
    return s


def window(d, from_ms=MEASURE_FROM_MS, tail_ms=1000):
    """Measured QPC window: first input at or after `from_ms` until `tail_ms`
    after the last real input (the final `wait` is not input)."""
    ins = [i for i in analyze.inputs(d) if i[2] != "wait"]
    if not ins:
        return 0, 1 << 62
    t0 = next((q for t, q, c in ins if t >= from_ms), ins[0][1])
    return t0, ins[-1][1] + int(tail_ms / 1000 * analyze.QPC)


def summarize(d, extra=None, from_ms=MEASURE_FROM_MS):
    m = analyze.meta(d)
    hz = analyze.refresh_hz(m)
    t0, t1 = window(d, from_ms)
    out = {"run": d.name, "mode": m["mode"], "config": m["config"], "refresh_hz": hz,
           "scale": m.get("scale"), "background_cpu_pct": m["background_cpu_pct"],
           "power_plan": m["power_plan"].split("(")[-1].rstrip(")"), **(extra or {})}
    if m["mode"] == "dd":
        cad = analyze.content_cadence(d, "list", t0, t1, hz)
        cf = analyze.clock_freezes(d)
        g = [x for q, x in cf.get("px_gui", []) if t0 <= q <= t1]
        T = 1000 / hz
        out.update({"cadence": cad, "gui_clock_p50_ms": analyze.pct(g, 50), "gui_clock_p99_ms": analyze.pct(g, 99),
                    "gui_clock_max_ms": max(g, default=None),
                    "gui_clock_skipped": sum(max(0, round(x / T) - 1) for x in g if x > 1.5 * T),
                    "frames": cf["frames"], "recorder_missed": cf["accumulated_gt1"]})
    if m["mode"] == "pm":
        out.update(analyze.frame_stats(analyze.presentmon(d), t0, t1, hz))
        rg = analyze.ring(d)
        if rg:
            attr = analyze.ring_attribution(rg)
            g, r, th = attr(t0, t1)
            out.update({"ring_gui_excess_max_ms": round(g, 2), "ring_render_excess_max_ms": round(r, 2)})
    with open(SUMMARY, "a") as f:
        f.write(json.dumps(out) + "\n")
    return out


def measure(name, cfg, script, mode="pm", extra=None, **kw):
    d = run(name, cfg, script, mode=mode, **kw)
    return summarize(d, extra)


def short(r):
    if r.get("mode") == "dd":
        c = r["cadence"]
        return {"run": r["run"], "list_updates_per_s": c["updates_per_s_in_motion"], "list_skipped": c["skipped_refreshes"],
                "list_skipped_per_min": c["skipped_per_min_of_motion"], "list_p99": c["interval_p99_ms"],
                "gui_clock_skipped": r["gui_clock_skipped"], "gui_clock_max": r["gui_clock_max_ms"],
                "recorder_missed": r["recorder_missed"]}
    keys = ["run", "frames", "expected_frames", "p50_ms", "p99_ms", "max_ms", "dropped", "dropped_per_min",
            "gpu_busy_p99_ms", "ring_gui_excess_max_ms", "ring_render_excess_max_ms"]
    return {k: r.get(k) for k in keys}


if __name__ == "__main__":
    print("import and call from a driver script")
