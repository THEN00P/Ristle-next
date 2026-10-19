"""144 Hz hiccups: classifies every on-screen drop in the recorded PresentMon
runs (no new measurement). Writes results/hiccups.json and prints a summary.

For each Qt run the vblank grid is recovered from the screen times of the
displayed frames (present time + MsUntilDisplayed); its phase is their
circular mean modulo the refresh period. Each present then gets a phase in
[0, 1) of the refresh interval. A drop (a displayed frame whose
MsBetweenDisplayChange exceeds 1.5 refreshes) is classified by what happened
since the previous displayed frame:

  late       a present gap above 1.5 refreshes: the app was late
  collision  a frame was presented and never displayed, with no late
             present: two presents fell into one composition interval and
             DWM showed only the newer one
  held       neither: every present was on time and displayed, but the frame
             reached the screen one refresh later than its predecessor's
             latency suggests (composition or GPU completion missed a vblank)
"""
import cmath, json, math, statistics
from collections import Counter, defaultdict
import analyze
from scenarios import SUMMARY, window
from spikelib import RUNS, ROOT

LIGHT = {"rows", "fling", "weight", "artwork", "churn", "cpu", "wheel", "touchpad"}


def light_runs():
    out = []
    for l in open(SUMMARY):
        r = json.loads(l)
        c = r["config"]
        if (r.get("mode") == "pm" and r.get("axis") in LIGHT and not c.get("QSG_RHI_PREFER_SOFTWARE_RENDERER")
                and c.get("QT_D3D_ADAPTER_INDEX") != "1" and (RUNS / r["run"] / "presentmon.csv").exists()):
            out.append(r)
    return out


def analyse(r):
    d = RUNS / r["run"]
    T = 1000 / r["refresh_hz"]
    t0, t1 = window(d)
    t1 = t0 + int(r["duration_s"] * analyze.QPC)
    fs = [f for f in analyze.presentmon(d) if t0 <= f["qpc"] <= t1]
    shown = [f for f in fs if f["MsUntilDisplayed"]]
    screen = [f["qpc"] / 1e4 + f["MsUntilDisplayed"] for f in shown]  # ms
    origin = cmath.phase(sum(cmath.exp(2j * math.pi * (s % T) / T) for s in screen)) / (2 * math.pi) * T
    phase = lambda q: ((q / 1e4 - origin) % T) / T
    # Residual of screen times against the grid (checks the grid fit).
    resid = [abs(((s - origin + T / 2) % T) - T / 2) for s in screen]
    intervals = [f["MsBetweenPresents"] for f in fs[1:]]
    kinds, drop_phases, all_phases = Counter(), [], [phase(f["qpc"]) for f in fs]
    latency = [f["MsUntilDisplayed"] / T for f in shown]
    idx = {id(f): i for i, f in enumerate(fs)}
    prev = None
    drops_at = []
    for f in shown:
        if prev is not None:
            gap = f["MsBetweenDisplayChange"]
            if gap and gap > 1.5 * T:
                between = fs[idx[id(prev)] + 1: idx[id(f)] + 1]
                n = round(gap / T) - 1
                if any(x["MsBetweenPresents"] > 1.5 * T for x in between):
                    k = "late"
                elif any(not x["MsUntilDisplayed"] for x in between):
                    k = "collision"
                    drop_phases += [phase(x["qpc"]) for x in between]
                else:
                    k = "held"
                kinds[k] += n
                drops_at.append(f["qpc"] / 1e7)
        prev = f
    return {"run": r["run"], "hz": r["refresh_hz"], "duration_s": r["duration_s"], "kinds": dict(kinds),
            "present_interval_mean": statistics.fmean(intervals), "present_interval_sd": statistics.pstdev(intervals),
            "refresh_ms": T, "grid_resid_p99_ms": sorted(resid)[int(0.99 * (len(resid) - 1))],
            "latency_refreshes": Counter(round(x) for x in latency), "phases": all_phases,
            "drop_phases": drop_phases, "drops_at": drops_at}


def hist(xs, bins=10):
    h = [0] * bins
    for x in xs:
        h[min(bins - 1, int(x * bins))] += 1
    tot = sum(h) or 1
    return [round(100 * v / tot, 1) for v in h]


def main():
    res = [analyse(r) for r in light_runs()]
    out = {}
    for key, sel in (("144", lambda x: x["hz"] > 100), ("60", lambda x: x["hz"] < 100)):
        rs = [x for x in res if sel(x)]
        k = Counter()
        lat = Counter()
        for x in rs:
            k.update(x["kinds"])
            lat.update(x["latency_refreshes"])
        dur = sum(x["duration_s"] for x in rs)
        spacing = [b - a for x in rs for a, b in zip(x["drops_at"], x["drops_at"][1:])]
        out[key] = {
            "runs": len(rs), "minutes": round(dur / 60, 1), "drops_by_kind": dict(k),
            "drops_per_min": round(sum(k.values()) / dur * 60, 1),
            "present_phase_hist_pct": hist([p for x in rs for p in x["phases"]]),
            "collision_present_phase_hist_pct": hist([p for x in rs for p in x["drop_phases"]]),
            "present_interval_mean_ms": round(statistics.fmean(x["present_interval_mean"] for x in rs), 4),
            "present_interval_sd_ms": round(statistics.fmean(x["present_interval_sd"] for x in rs), 3),
            "refresh_ms": round(statistics.fmean(x["refresh_ms"] for x in rs), 4),
            "grid_resid_p99_ms": round(max(x["grid_resid_p99_ms"] for x in rs), 3),
            "latency_refreshes_pct": {kk: round(100 * v / sum(lat.values()), 1) for kk, v in sorted(lat.items())},
            "drop_spacing_s_median": round(statistics.median(spacing), 2) if spacing else None,
        }
    (ROOT / "results" / "hiccups.json").write_text(json.dumps(out, indent=1))
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
