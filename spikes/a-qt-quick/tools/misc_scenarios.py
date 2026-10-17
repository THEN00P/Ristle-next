"""Scenario 5 (text sharpness), 6 (MultiEffect glow cost) and 7 (idle memory,
30-minute soak). Usage: misc_scenarios.py <tag> text|glow|idle|soak"""
import csv, json, statistics, subprocess, sys, time
import analyze
from scenarios import MEASURE_FROM_MS, ORIGIN, SUMMARY, LIST_X, LIST_TOP, LIST_BOTTOM, measure, scroll_script, short
from spikelib import HARNESS, RUNS, Script, TITLE, display, run, wait_window, sh
from ramps import WARP, IGPU

BASE = {"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": 1}


def text(tag):
    """Duplicated-frame crops of the Qt specimen and of the native reference."""
    out = {}
    d = run(f"{tag}_text_qt", {"SPIKE_SPECIMEN": 1}, None, mode="dd", tail=2.0,
            rects={"window": (0, 0, 1280, 720)}, snaps=[1200], clocks=False)
    out["qt"] = str(d)

    for mode in ("cleartype", "grayscale", "gdi"):
        def launch(mode=mode):
            p = subprocess.Popen([str(HARNESS), "textref", "8", mode])
            return p, "Ristle text reference"
        d = run(f"{tag}_text_native_{mode}", {}, None, mode="dd", tail=2.0, launch=launch,
                rects={"window": (0, 0, 1280, 720)}, snaps=[1200], clocks=False, close=False)
        out[mode] = str(d)
        time.sleep(7)  # the reference window closes itself
    print(out)


def glow(tag):
    steps = [({**BASE, "SPIKE_GLOW": g}, None) for g in (0, 1, 2)] + \
            [({**BASE, "SPIKE_GLOW": g, **IGPU}, None) for g in (0, 1, 2)] + \
            [({**BASE, "SPIKE_GLOW": g, **WARP}, None) for g in (0, 1, 2)] + \
            [({**BASE, "SPIKE_GLOW": 2, "SPIKE_WEIGHT": 2, **WARP}, None)]
    for i, (cfg, aff) in enumerate(steps):
        r = measure(f"{tag}_glow_{i}", cfg, scroll_script("fling", 8000), affinity=aff,
                    extra={"axis": "glow", "step": i, "scenario": "glow"})
        print("glow", i, cfg, short(r), r["gpu_busy_p50_ms"], flush=True)


def idle(tag):
    """Idle memory with the 1,000-row page open, no input, small ring."""
    for name, cfg in (("idle_1k", {"SPIKE_ROWS": 1000, "SPIKE_RING": 4096}),
                      ("idle_1k_noheartbeat", {"SPIKE_ROWS": 1000, "SPIKE_RING": 4096, "SPIKE_HEARTBEAT": 0})):
        d = run(f"{tag}_{name}", cfg, None, mode="pm", settle=10, tail=30)
        rows = list(csv.DictReader(open(d / "proc.csv")))
        res = {"run": d.name, "config": cfg,
               "private_mb_median": statistics.median(float(r["private_mb"]) for r in rows),
               "working_set_mb_median": statistics.median(float(r["working_set_mb"]) for r in rows),
               "cpu_pct_median": statistics.median(float(r["proc_cpu_pct"]) for r in rows)}
        with open(SUMMARY.parent / "memory.jsonl", "a") as f:
            print(json.dumps({"tag": tag, **res}), file=f)
        print(res, flush=True)


def soak_script(minutes, k):
    """Repeating 60 s cycle: flings, queue edits, page switches, sidebar."""
    X = lambda v: round((ORIGIN[0] + v) * k)
    Y = lambda v: round((ORIGIN[1] + v) * k)
    s = Script()
    s.add(0, "focus", TITLE)
    s.touch_drag(100, X(LIST_X), Y(LIST_BOTTOM - 20), Y(LIST_BOTTOM - 40), 60)
    for m in range(minutes):
        t = MEASURE_FROM_MS + m * 60000
        for i in range(10):  # 20 s of flings, alternating direction
            up = i % 4 < 2
            y0, y1 = (LIST_BOTTOM, LIST_BOTTOM - 400) if up else (LIST_TOP, LIST_TOP + 400)
            s.touch_drag(t + i * 2000, X(LIST_X), Y(y0), Y(y1), 50)
        t += 20000
        s.key(t, "F2")            # 10 s of queue edits by the core
        s.key(t + 10000, "F2")
        t += 11000
        for i in range(8):        # page switches
            s.key(t + i * 1200, "2" if i % 2 == 0 else "1", ctrl=True)
        t += 10000
        for i in range(6):        # sidebar
            s.key(t + i * 900, "B", ctrl=True)
        t += 6000
        s.add(t, "move", X(600), Y(450))
        for i in range(30):       # wheel
            s.add(t + 100 + i * 100, "wheel", -120 if i < 15 else 120)
    return s


def soak(tag, minutes=30):
    k = int(display()["scale"]) / 100
    cfg = {**BASE, "SPIKE_QUEUE_OPS": 5, "SPIKE_RING": 1 << 22}
    d = run(f"{tag}_soak", cfg, soak_script(minutes, k), mode="pm", tail=2)
    print(d)


if __name__ == "__main__":
    tag = sys.argv[1]
    for which in sys.argv[2:]:
        globals()[which](tag)
