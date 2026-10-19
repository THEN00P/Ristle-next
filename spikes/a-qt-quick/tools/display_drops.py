"""Display drops next to present drops for every PresentMon run in
results/summary.jsonl (Codex review, final round).

A present drop is a gap between two Present calls longer than 1.5 refreshes.
A display drop is the same for `MsBetweenDisplayChange`: the time between two
frames reaching the screen. When the compositor discards a frame while the
app keeps presenting every refresh, only the second shows it. Writes
results/display_drops.jsonl (one line per run) for report_data.py.
"""
import json
import analyze
from scenarios import SUMMARY, window
from spikelib import RUNS, ROOT

OUT = ROOT / "results" / "display_drops.jsonl"


def display_stats(d, t0, t1, hz):
    T = 1000 / hz
    fs = [f for f in analyze.presentmon(d) if t0 <= f["qpc"] <= t1]
    shown = [f for f in fs if f["MsUntilDisplayed"]]
    bd = [f["MsBetweenDisplayChange"] for f in shown[1:] if f["MsBetweenDisplayChange"]]
    drops = sum(max(0, round(x / T) - 1) for x in bd if x > 1.5 * T)
    bp = [f["MsBetweenPresents"] for f in fs[1:]]
    present = sum(max(0, round(x / T) - 1) for x in bp if x > 1.5 * T)
    dur = (t1 - t0) / analyze.QPC
    return {"display_dropped": drops, "display_dropped_per_min": round(drops / dur * 60, 2) if dur else None,
            "display_max_ms": max(bd, default=None), "not_displayed": len(fs) - len(shown),
            "present_dropped_check": present}


def main():
    rows = [json.loads(l) for l in open(SUMMARY)]
    seen = set()
    with open(OUT, "w") as f:
        for r in rows:
            d = RUNS / r["run"]
            if r.get("mode") != "pm" or r["run"] in seen or not (d / "presentmon.csv").exists():
                continue
            seen.add(r["run"])
            t0, t1 = window(d)
            if r.get("duration_s"):
                # Same window as the stored summary (soaks used their own).
                t1 = t0 + int(r["duration_s"] * analyze.QPC)
            out = {"run": r["run"], "dropped": r.get("dropped"), "dropped_per_min": r.get("dropped_per_min"),
                   **display_stats(d, t0, t1, r["refresh_hz"])}
            print(json.dumps(out), file=f)


if __name__ == "__main__":
    main()
