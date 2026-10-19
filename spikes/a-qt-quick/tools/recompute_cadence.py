"""Re-derives every duplication-pass cadence figure with the corrected metric
(analyze.content_cadence: long freezes inside scripted input windows count).
Keeps the old figures as `cadence_v1` and prints what changed.
Usage: recompute_cadence.py"""
import json
import analyze
from scenarios import window
from spikelib import ROOT, RUNS

R = ROOT / "results"


def recompute(name):
    d = RUNS / name
    t0, t1 = window(d)
    return analyze.content_cadence(d, "list", t0, t1)


def main():
    changes = []
    rows = [json.loads(l) for l in open(R / "summary.jsonl")]
    for r in rows:
        if r.get("mode") == "dd" and "cadence" in r and (RUNS / r["run"]).exists():
            r.setdefault("cadence_v1", r["cadence"])
            r["cadence"] = recompute(r["run"])
            changes.append((r["run"], r["cadence_v1"], r["cadence"]))
    with open(R / "summary.jsonl", "w") as f:
        for r in rows:
            print(json.dumps(r), file=f)
    rows = [json.loads(l) for l in open(R / "baselines.jsonl")]
    for r in rows:
        name = f"{r['tag']}_baseline_{r['app']}_{r['input'].replace(':', '')}_dd"
        if (RUNS / name).exists():
            r.setdefault("cadence_v1", r["cadence"])
            r["cadence"] = recompute(name)
            changes.append((name, r["cadence_v1"].get("active", r["cadence_v1"]), r["cadence"]))
    with open(R / "baselines.jsonl", "w") as f:
        for r in rows:
            print(json.dumps(r), file=f)
    for name, old, new in changes:
        o = old.get("skipped_per_min_of_motion")
        print(f"{name}: skipped/min {o} -> {new['skipped_per_min']}, updates/s "
              f"{old.get('updates_per_s_in_motion')} -> {new['updates_per_s']}, longest freeze {new['longest_freeze_ms']} ms")


if __name__ == "__main__":
    main()
