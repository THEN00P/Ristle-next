"""Scenario 1: animated queue insert, remove and drag reorder, with a ramp of
edit rate. Each step runs a PresentMon pass and a duplication pass.
Usage: queue_scenario.py <condition-tag> [rates...]"""
import json, sys
import analyze, queue_check
from scenarios import ORIGIN, SUMMARY, measure, summarize, short, MEASURE_FROM_MS
from spikelib import Script, TITLE, display, run

LIST_LEFT, LIST_TOP, LIST_W, LIST_H = 964, 178, 296, 522
ROW = 52
TRANSITION_MS = 220


def script(rate, k):
    X = lambda v: round((ORIGIN[0] + v) * k)
    Y = lambda v: round((ORIGIN[1] + v) * k)
    row_y = lambda i: LIST_TOP + ROW * i + ROW // 2
    s = Script()
    s.add(0, "focus", TITLE)
    s.add(300, "move", X(1100), Y(row_y(3)))
    t = MEASURE_FROM_MS
    # Drag reorders: row 1 down to row 5 and back, twice.
    for _ in range(2):
        s.mouse_drag(t, X(1100), Y(row_y(1)), X(1100), Y(row_y(5)), 500)
        t += 1100
        s.mouse_drag(t, X(1100), Y(row_y(5)), X(1100), Y(row_y(1)), 500)
        t += 1100
    # Random edits by the core at `rate` per second for 6 s (F2 toggles).
    s.key(t, "F2")
    t += 6000
    s.key(t, "F2")
    t += 800
    # Bursts that interrupt running transitions (the QTBUG-49868 pattern).
    for i in range(12):
        s.key(t + i * 50, ["INS", "DEL", "M"][i % 3], ctrl=(i % 3 == 2))
    t += 12 * 50 + 800
    s.add(t, "move", X(1100), Y(row_y(3)))
    return s


def drag_windows(d):
    ins = analyze.inputs(d)
    downs = [q for t, q, c in ins if c == "ldown"]
    ups = [q for t, q, c in ins if c == "lup"]
    return [(a, b + int((TRANSITION_MS + 50) * 1e4)) for a, b in zip(downs, ups)]


def run_rate(tag, rate, fix=1):
    k = int(display()["scale"]) / 100
    cfg = {"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": 1, "SPIKE_QUEUE_OPS": rate, "SPIKE_TRANSITION_MS": TRANSITION_MS,
           "SPIKE_QUEUE_FIX": fix}
    sfx = "" if fix else "_plain"
    pm = measure(f"{tag}_queue{sfx}_{rate}_pm", cfg, script(rate, k),
                 extra={"axis": "queue_ops" + sfx, "step": rate, "scenario": "queue"})
    d = run(f"{tag}_queue{sfx}_{rate}_dd", cfg, script(rate, k), mode="dd",
            rects={"queue": (LIST_LEFT, LIST_TOP, LIST_W, LIST_H)},
            marker=(LIST_LEFT, LIST_TOP, LIST_TOP + LIST_H))
    qc = queue_check.check(d, TRANSITION_MS, round(ROW * k), drag_windows(d))
    qc["latency"] = analyze.input_to_photon(d, "queue", lambda c: c.startswith("key 45 down") or c.startswith("key 46 down"))
    with open(SUMMARY.parent / "queue_checks.jsonl", "a") as f:
        print(json.dumps({"tag": tag, "rate": rate, "documented_settling": bool(fix), **qc}), file=f)
    print(rate, short(pm), qc, flush=True)
    return pm, qc


if __name__ == "__main__":
    tag = sys.argv[1]
    fix = 0 if "plain" in sys.argv[2:] else 1
    for rate in [int(x) for x in sys.argv[2:] if x.isdigit()] or [1, 5, 20, 50, 100, 200]:
        run_rate(tag, rate, fix)
