"""Scenario 3 (sidebar toggle over a large list) and scenario 4 (switching
between two heavy pages, cached and fresh). Each step: PresentMon pass, then a
duplication pass for input-to-photon.
Usage: ui_scenarios.py <condition-tag> sidebar|pages"""
import json, sys
import analyze
from scenarios import ORIGIN, SUMMARY, MEASURE_FROM_MS, measure, short
from spikelib import Script, TITLE, display, run
from ramps import WARP

PAGE_RECT = (240, 8, 700, 690)       # centre page area (logical)
SIDEBAR_RECT = (0, 0, 232, 700)

SIDEBAR_STEPS = [({"SPIKE_ROWS": n, "SPIKE_WEIGHT": w}, None) for n, w in ((1000, 1), (1000000, 1), (1000000, 2))] + \
                [({"SPIKE_ROWS": 1000000, "SPIKE_WEIGHT": 2}, 0x1), ({"SPIKE_ROWS": 1000000, "SPIKE_WEIGHT": 2, **WARP}, None),
                 ({"SPIKE_ROWS": 1000000, "SPIKE_WEIGHT": 2, **WARP}, 0x1)]
PAGE_STEPS = [({"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": w, "SPIKE_FRESH_PAGES": f}, None) for f in (0, 1) for w in (1, 2)] + \
             [({"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": 2, "SPIKE_FRESH_PAGES": 1, "SPIKE_ART_CACHE": 0}, None),
              ({"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": 2, "SPIKE_FRESH_PAGES": 1}, 0x1),
              ({"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": 2, "SPIKE_FRESH_PAGES": 1, **WARP}, None)]


def sidebar_script():
    s = Script()
    s.add(0, "focus", TITLE)
    for i in range(12):
        s.key(MEASURE_FROM_MS + i * 700, "B", ctrl=True)
    return s


def pages_script():
    s = Script()
    s.add(0, "focus", TITLE)
    for i in range(12):
        s.key(MEASURE_FROM_MS + i * 900, "2" if i % 2 == 0 else "1", ctrl=True)
    return s


def run_steps(tag, which):
    steps, script, rect, keys = {
        "sidebar": (SIDEBAR_STEPS, sidebar_script, SIDEBAR_RECT, ("key 66 down",)),
        "pages": (PAGE_STEPS, pages_script, PAGE_RECT, ("key 49 down", "key 50 down")),
    }[which]
    for i, (cfg, affinity) in enumerate(steps):
        pm = measure(f"{tag}_{which}_{i}_pm", cfg, script(), affinity=affinity,
                     extra={"axis": which, "step": i, "affinity": affinity, "scenario": which})
        d = run(f"{tag}_{which}_{i}_dd", cfg, script(), mode="dd", affinity=affinity,
                rects={"target": rect, "list": (248, 200, 680, 480)})
        lat = analyze.input_to_photon(d, "target", lambda c: c.startswith(keys))
        with open(SUMMARY.parent / "latency.jsonl", "a") as f:
            print(json.dumps({"tag": tag, "scenario": which, "step": i, "config": cfg, "affinity": affinity,
                              **{k: v for k, v in lat.items()}}), file=f)
        print(which, i, cfg, affinity, short(pm), {k: lat[k] for k in ("n", "p50_ms", "p99_ms", "max_ms")}, flush=True)


if __name__ == "__main__":
    tag = sys.argv[1]
    for which in sys.argv[2:] or ["sidebar", "pages"]:
        run_steps(tag, which)
