"""Re-runs queue_check on every 2026-10-19 queue capture after the checker
gained the missing-rows test (Codex review, final round). No new
measurement: reads the 2026-10-19 captures (runs/fixq*) and the matrix's
queue scenario captures (runs/*_queue*_dd). Writes results/queue_recheck.jsonl."""
import json
import analyze, queue_check, queue_scenario as Q
from spikelib import RUNS, ROOT

OUT = ROOT / "results" / "queue_recheck.jsonl"


def main():
    with open(OUT, "w") as f:
        dirs = sorted(set(RUNS.glob("fixq*")) | {d for d in RUNS.glob("*_queue*_dd") if not d.name.startswith("mon")})
        for d in dirs:
            if not (d / "dupcap.csv").exists():
                continue
            k = analyze.meta(d).get("scale") or 1.0
            scenario = "_queue_" in d.name
            qc = queue_check.check(d, Q.TRANSITION_MS, round(Q.ROW * k), Q.drag_windows(d) if scenario else [])
            out = {"run": d.name, "scenario": scenario, "frames": qc["frames"], "settled_frames": qc["settled_frames"],
                   "by_kind": qc["by_kind"], "frames_with_problems": qc["frames_with_problems"],
                   "persistent_at_end": qc["persistent_at_end"], "recorder_missed": qc["recorder_missed"],
                   "examples": [e for e in qc["examples"] if "missing-rows" in e][:3]}
            print(json.dumps(out), file=f)
            print(d.name, out["frames"], out["settled_frames"], out["by_kind"], flush=True)


if __name__ == "__main__":
    main()
