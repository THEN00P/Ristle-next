"""Queue corruption diagnosis: one duplication run with snapshots after the
edits stop, the measured edit rate from the ring buffer, and the checker.
Usage: queue_diag.py <name> <rate|burst:n,gap,period> [fix 0|1|2] [secs]"""
import json, sys
import analyze, queue_check, queue_scenario as Q
from scenarios import MEASURE_FROM_MS, ORIGIN
from spikelib import Script, TITLE, display, run


def churn_script(k, secs):
    X = lambda v: round((ORIGIN[0] + v) * k)
    Y = lambda v: round((ORIGIN[1] + v) * k)
    s = Script()
    s.add(0, "focus", TITLE)
    s.add(300, "move", X(700), Y(400))     # pointer away from the queue
    s.key(MEASURE_FROM_MS, "F2")
    s.key(MEASURE_FROM_MS + secs * 1000, "F2")
    s.add(MEASURE_FROM_MS + secs * 1000 + 1500, "move", X(700), Y(420))
    return s


def main(name, load, fix=1, secs=8, snaps_after=(600, 1200)):
    k = int(display()["scale"]) / 100
    cfg = {"SPIKE_ROWS": 1000, "SPIKE_WEIGHT": 1, "SPIKE_TRANSITION_MS": Q.TRANSITION_MS, "SPIKE_QUEUE_FIX": fix}
    if load.startswith("burst:"):
        n, gap, period = (int(x) for x in load[6:].split(","))
        cfg.update({"SPIKE_BURST_COUNT": n, "SPIKE_BURST_GAP_MS": gap, "SPIKE_BURST_PERIOD_MS": period})
    else:
        cfg["SPIKE_QUEUE_OPS"] = int(load)
    # dupcap starts about 0.8 s before the script.
    end_ms = 800 + MEASURE_FROM_MS + secs * 1000
    d = run(name, cfg, churn_script(k, secs), mode="dd", tail=2.5,
            rects={"queue": (Q.LIST_LEFT, Q.LIST_TOP, Q.LIST_W, Q.LIST_H)},
            marker=(Q.LIST_LEFT, Q.LIST_TOP, Q.LIST_TOP + Q.LIST_H),
            snaps=[end_ms + a for a in snaps_after])
    rg = analyze.ring(d)
    edits = [q for q, kk, a, t in rg["events"] if kk in (13, 14, 15)]
    rate = (len(edits) - 1) / ((edits[-1] - edits[0]) / 1e7) if len(edits) > 1 else 0
    qc = queue_check.check(d, Q.TRANSITION_MS, round(Q.ROW * k), [])
    out = {"run": name, "load": load, "fix": fix, "edits": len(edits), "measured_rate": round(rate, 1),
           "problems": qc["problems"], "by_kind": qc["by_kind"], "persistent_at_end": qc["persistent_at_end"],
           "examples": qc["examples"][:4]}
    print(json.dumps(out), flush=True)
    return d, out


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 1,
         int(sys.argv[4]) if len(sys.argv) > 4 else 8)
