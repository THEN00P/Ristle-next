"""Analysis for spike A runs. PresentMon is the timing evidence; the ring
buffer only attributes; duplication passes give content cadence, clocks,
corruption and input-to-photon."""
from __future__ import annotations

import csv, json, pathlib, statistics

EV = dict(GUI_ANIM=1, SYNC_B=2, SYNC_E=3, REND_B=4, REND_E=5, SWAP=6, STALL_GUI_B=7, STALL_GUI_E=8,
          STALL_REND_B=9, STALL_REND_E=10, INPUT=11, MARK=12, Q_INS=13, Q_REM=14, Q_MOVE=15,
          PAGE=16, ART=17, Q_RESET=18)
QPC = 1e7  # this machine: 10 MHz (checked against every file header)


def _header(path):
    first = open(path, encoding="utf-8-sig").readline()
    return dict(p.split("=", 1) for p in first.lstrip("# ").split()) if first.startswith("#") else {}


def ring(d):
    p = pathlib.Path(d) / "ring.csv"
    if not p.exists():
        return None
    h = _header(p)
    assert int(h["qpc_freq"]) == QPC
    rows = [l.split(",") for l in open(p) if l[0].isdigit()]
    ev = [(int(q), int(k), int(a), int(t)) for q, k, a, t in rows]
    return {"gui_tid": int(h["gui_tid"]), "dropped": int(h["dropped"]), "events": ev}


def presentmon(d, app=None):
    p = pathlib.Path(d) / "presentmon.csv"
    rows = list(csv.DictReader(open(p, encoding="utf-8-sig")))
    if app:
        rows = [r for r in rows if r["Application"] == app]
    out = []
    for r in rows:
        f = {"qpc": int(r["TimeInQPC"]), "cpu_start": int(r["CPUStartQPC"]), "mode": r["PresentMode"]}
        for k in ("MsBetweenPresents", "MsBetweenDisplayChange", "MsCPUBusy", "MsCPUWait", "MsUntilDisplayed",
                  "MsGPUBusy", "MsGPUTime", "MsInPresentAPI"):
            v = r.get(k, "NA")
            f[k] = float(v) if v not in ("NA", "") else None
        out.append(f)
    return out


def inputs(d):
    p = pathlib.Path(d) / "input.csv"
    if not p.exists():
        return []
    rows = [l.rstrip("\n").split(",", 3) for l in open(p) if l[0].isdigit()]
    return [(float(t), int(q), c) for _, t, q, c in rows]


def dupcap(d):
    p = pathlib.Path(d) / "dupcap.csv"
    h = _header(p)
    rd = csv.reader(l for l in open(p) if not l.startswith("#"))
    cols = next(rd)
    rows = [dict(zip(cols, r)) for r in rd]
    for r in rows:
        r["present_qpc"] = int(r["present_qpc"])
        r["accumulated"] = int(r["accumulated"])
    return h, cols, rows


def meta(d):
    return json.load(open(pathlib.Path(d) / "meta.json"))


def refresh_hz(m):
    if "refresh_exact" in m["display"]:
        return float(m["display"]["refresh_exact"])
    hz = int(m["display"]["refresh"])
    return 59.951 if hz == 59 else (143.998 if hz in (143, 144) else float(hz))


def pct(xs, p):
    xs = sorted(xs)
    if not xs:
        return None
    i = min(len(xs) - 1, max(0, round(p / 100 * (len(xs) - 1))))
    return xs[i]


def frame_stats(frames, t0, t1, hz):
    """Frame-time stats for presents in [t0, t1] (QPC). `dropped` counts
    present intervals longer than 1.5 refresh periods (the vsyncs missed);
    `display_dropped` counts the same for the intervals between frames
    reaching the screen, which is what decides (frame-timing method, rule 1)."""
    T = 1000 / hz
    fs = [f for f in frames if t0 <= f["qpc"] <= t1]
    bp = [f["MsBetweenPresents"] for f in fs[1:]]
    bd = [f["MsBetweenDisplayChange"] for f in fs[1:] if f["MsBetweenDisplayChange"]]
    drops = sum(max(0, round(x / T) - 1) for x in bp if x > 1.5 * T)
    shown = [f for f in fs if f["MsUntilDisplayed"]]
    bs = [f["MsBetweenDisplayChange"] for f in shown[1:] if f["MsBetweenDisplayChange"]]
    display_drops = sum(max(0, round(x / T) - 1) for x in bs if x > 1.5 * T)
    dur_s = (t1 - t0) / QPC
    return {
        "frames": len(fs), "expected_frames": round(dur_s * hz), "duration_s": round(dur_s, 3),
        "p50_ms": pct(bp, 50), "p99_ms": pct(bp, 99), "max_ms": max(bp, default=None),
        "display_p99_ms": pct(bd, 99), "display_max_ms": max(bd, default=None),
        "dropped": drops, "dropped_per_min": round(drops / dur_s * 60, 2) if dur_s else None,
        "display_dropped": display_drops,
        "display_dropped_per_min": round(display_drops / dur_s * 60, 2) if dur_s else None,
        "until_displayed_p50_ms": pct([f["MsUntilDisplayed"] for f in fs if f["MsUntilDisplayed"]], 50),
        "gpu_busy_p50_ms": pct([f["MsGPUBusy"] for f in fs if f["MsGPUBusy"] is not None], 50),
        "gpu_busy_p99_ms": pct([f["MsGPUBusy"] for f in fs if f["MsGPUBusy"] is not None], 99),
        "cpu_busy_p99_ms": pct([f["MsCPUBusy"] for f in fs if f["MsCPUBusy"] is not None], 99),
        "modes": sorted({f["mode"] for f in fs}),
    }


def phases(rg):
    """Per-frame GUI and render phase durations from the ring (ms), keyed by start qpc."""
    gui, rend, anim = [], [], []
    last_anim = None
    last_rb = None
    for q, k, a, t in rg["events"]:
        if k == EV["GUI_ANIM"]:
            if last_anim is not None:
                anim.append((last_anim, (q - last_anim) / 1e4))
            last_anim = q
        elif k == EV["SYNC_B"] and last_anim is not None:
            gui.append((last_anim, (q - last_anim) / 1e4))
        elif k == EV["REND_B"]:
            last_rb = q
        elif k == EV["REND_E"] and last_rb is not None:
            rend.append((last_rb, (q - last_rb) / 1e4))
    return gui, rend, anim


def ring_attribution(rg):
    """Returns f(lo, hi) -> (gui_excess_ms, render_excess_ms, thread) for a
    window, from the ring's per-frame GUI and render phase durations."""
    gui, rend, anim = phases(rg)
    med_gui = statistics.median(x for _, x in gui)
    med_rend = statistics.median(x for _, x in rend)
    med_anim = statistics.median(x for _, x in anim)

    def f(lo, hi):
        g = max([x - med_gui for s, x in gui if lo <= s <= hi] +
                [x - med_anim for s, x in anim if lo <= s <= hi] + [0])
        r = max([x - med_rend for s, x in rend if lo <= s <= hi] + [0])
        return g, r, ("gui" if g > r else "render")
    return f


def injected(rg):
    for q, k, a, t in rg["events"]:
        if k in (EV["STALL_GUI_B"], EV["STALL_REND_B"]):
            yield q, ("gui" if k == EV["STALL_GUI_B"] else "render"), a


def control(d, hz=None):
    """Positive control, PresentMon pass: every injected stall must be found
    by PresentMon with its duration within one refresh interval, and
    attributed to the right thread by the ring buffer."""
    m = meta(d)
    hz = hz or refresh_hz(m)
    T = 1000 / hz
    rg = ring(d)
    frames = presentmon(d)
    med = statistics.median(f["MsBetweenPresents"] for f in frames[1:])
    attr = ring_attribution(rg)
    stalls = []
    for q, thread, a in injected(rg):
        lo, hi = q - int(0.05 * QPC), q + int((a / 1000 + 0.12) * QPC)
        est = max((f["MsBetweenPresents"] - med for f in frames if lo <= f["qpc"] <= hi), default=0.0)
        g_ex, r_ex, attributed = attr(lo, hi)
        stalls.append({"thread": thread, "injected_ms": a, "measured_ms": round(est, 2),
                       "error_ms": round(est - a, 2), "ring_gui_excess_ms": round(g_ex, 2),
                       "ring_render_excess_ms": round(r_ex, 2), "attributed": attributed,
                       "detected": abs(est - a) <= T and attributed == thread})
    return {"run": pathlib.Path(d).name, "source": "presentmon", "refresh_hz": hz,
            "median_frame_ms": round(med, 3), "frames": len(frames), "stalls": stalls}


def clock_freezes(d):
    """Duplication pass: intervals between changes of each clock pixel."""
    _, cols, rows = dupcap(d)
    out = {}
    for name in [c for c in cols if c.startswith("px_")]:
        freezes, last_change, last_val = [], None, None
        for r in rows:
            if r[name] != last_val:
                if last_change is not None:
                    freezes.append((last_change, (r["present_qpc"] - last_change) / 1e4))
                last_change, last_val = r["present_qpc"], r[name]
        out[name] = freezes
    out["accumulated_gt1"] = sum(1 for r in rows if r["accumulated"] > 1)
    out["frames"] = len(rows)
    return out


def control_dd(d, hz=None):
    """Positive control, duplication pass: each stall must show as a freeze
    of the GUI-thread clock on screen, with the ring attributing the thread."""
    m = meta(d)
    hz = hz or refresh_hz(m)
    T = 1000 / hz
    rg = ring(d)
    cf = clock_freezes(d)
    base = statistics.median(x for _, x in cf["px_gui"])
    attr = ring_attribution(rg)
    res = []
    for q, thread, a in injected(rg):
        lo, hi = q - int(0.03 * QPC), q + int((a / 1000 + 0.15) * QPC)
        g = max((x for s, x in cf["px_gui"] if lo <= s <= hi), default=0.0) - base
        g_ex, r_ex, attributed = attr(lo, hi)
        res.append({"thread": thread, "injected_ms": a, "measured_ms": round(g, 2), "error_ms": round(g - a, 2),
                    "attributed": attributed, "detected": abs(g - a) <= T and attributed == thread})
    return {"run": pathlib.Path(d).name, "source": "desktop duplication (GUI clock)", "refresh_hz": hz,
            "median_frame_ms": round(base, 3), "frames": cf["frames"],
            "missed_by_recorder": cf["accumulated_gt1"], "stalls": res}


def active_windows(d, after_ms=200, wheel_gap_ms=300):
    """Scripted input phases in which the content must keep moving: a touch
    drag from finger down until `after_ms` after release (later, a fling's
    tail moves less than a pixel per frame, so an unchanged frame there is not
    a stall), a wheel or touchpad stream from its first to its last event plus
    `after_ms`, and a mouse drag from press to release."""
    out, down, wheel = [], None, None
    for t, q, c in inputs(d):
        if c.startswith("touch down") or c == "ldown":
            down = q
        elif (c.startswith("touch up") or c == "lup") and down is not None:
            out.append((down, q + int(after_ms * 1e4)))
            down = None
        elif c.startswith("wheel"):
            if wheel and q - wheel[1] <= wheel_gap_ms * 1e4:
                wheel[1] = q
            else:
                if wheel:
                    out.append((wheel[0], wheel[1] + int(after_ms * 1e4)))
                wheel = [q, q]
    if wheel:
        out.append((wheel[0], wheel[1] + int(after_ms * 1e4)))
    return out


def content_cadence(d, region, t0=0, t1=1 << 62, hz=None, windows=None):
    """Duplication pass: how smoothly `region` updated while scripted input
    was moving it. Inside each active window (`active_windows`) every interval
    between two consecutive changes counts, however long: content that froze
    and then moved again was stalled. The time after the last change in a
    window is not counted, because content that stops for good stopped on
    purpose (a touch catching a fling stops it, and a short reversed drag does
    not start a new one). An interval longer than 1.5 refresh periods is a
    hitch; it counts the refreshes the content skipped. Fling tails outside
    the windows are not measured. Works for any app, so the baselines
    compare."""
    m = meta(d)
    hz = hz or refresh_hz(m)
    T = 1000 / hz
    _, cols, rows = dupcap(d)
    windows = active_windows(d) if windows is None else windows
    changes, prev = [], None
    for r in rows:
        if prev is not None and r[region] != prev:
            changes.append(r["present_qpc"])
        prev = r[region]
    last_frame = rows[-1]["present_qpc"] if rows else 0
    iv, active_s, n_changes = [], 0.0, 0
    for w0, w1 in windows:
        w0, w1 = max(w0, t0), min(w1, t1, last_frame)
        if w1 <= w0:
            continue
        active_s += (w1 - w0) / 1e7
        inside = [c for c in changes if w0 <= c <= w1]
        n_changes += len(inside)
        # Before the first change is input latency; after the last change the
        # content has stopped. Neither is a stall.
        for a, b in zip(inside, inside[1:]):
            iv.append((b - a) / 1e4)
    skipped = sum(max(0, round(x / T) - 1) for x in iv if x > 1.5 * T)
    moving_s = sum(iv) / 1000
    return {"changes": n_changes, "active_s": round(active_s, 2), "moving_s": round(moving_s, 2),
            "updates_per_s": round(len(iv) / moving_s, 1) if moving_s else None,
            "interval_p50_ms": pct(iv, 50), "interval_p99_ms": pct(iv, 99),
            "longest_freeze_ms": round(max(iv, default=0), 1),
            "skipped_refreshes": skipped,
            "skipped_per_min": round(skipped / moving_s * 60, 1) if moving_s else None,
            "recorder_missed": sum(1 for r in rows if r["accumulated"] > 1)}


def input_to_photon(d, region, select):
    """For each input whose command matches `select`, the time from the
    SendInput return (QPC) to the DWM present time of the first duplicated
    frame in which `region` changed. Both clocks are QPC, outside the app."""
    _, cols, rows = dupcap(d)
    lat = []
    for t, q, c in inputs(d):
        if not select(c):
            continue
        prev = None
        for r in rows:
            if r["present_qpc"] <= q:
                prev = r[region]
                continue
            if prev is not None and r[region] != prev:
                lat.append(round((r["present_qpc"] - q) / 1e4, 2))
                break
            prev = r[region]
    return {"n": len(lat), "p50_ms": pct(lat, 50), "p99_ms": pct(lat, 99), "max_ms": max(lat, default=None),
            "samples": lat}


if __name__ == "__main__":
    import sys
    cmd, d = sys.argv[1], sys.argv[2]
    r = {"control": control, "control_dd": control_dd}[cmd](d)
    print(json.dumps(r, indent=1))
