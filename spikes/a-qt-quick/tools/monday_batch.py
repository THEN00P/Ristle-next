"""Monday batch (2026-10-19): the measurements the Codex round 1 review and the
user's decisions left open. Unattended, about 4.5 hours, at 144 Hz / 100 %
with the D3D12 backend (the candidate production configuration). Restores the
display mode it found and writes results/monday_summary.md.

1. Queue corruption: transition variants 0 to 4 against low-rate bursts (2 to
   5 edits inside one transition, as in a Party) and against paced rates of
   5 to 200 edits/s; then the full queue scenario (drag reorders and keyboard
   bursts) for the variants that came out clean.
2. Re-runs of the steps whose rate was mislabelled before pacing: the churn
   axis and the 100 and 200 edits/s queue steps.
3. Fresh page creation with asynchronous incubation and a skeleton, against
   the synchronous build: dropped frames and time until the page is populated.
4. A 3-hour soak with a memory log every 10 s (private bytes, Win32 heap
   committed and allocated, GPU local memory) and the QML garbage collector's
   statistics, to tell caches from a leak.

Usage (from spikes/a-qt-quick/tools, nothing else running):
    C:\\Users\\loren\\spike-a-tools\\venv\\Scripts\\python.exe -u monday_batch.py
Steps can be picked: monday_batch.py queue rates pages soak (motion: the
full queue scenario for variant 4; MONDAY_TAG=<tag> keeps a re-run apart)
"""
import csv, json, os, statistics, subprocess, sys, time, traceback
import analyze, queue_check, queue_diag, queue_scenario as Q, ramps, spikelib
import misc_scenarios, ui_scenarios
from matrix import set_condition
from scenarios import SUMMARY, measure, short
from spikelib import RUNS, ROOT, display, harness, run

# MONDAY_TAG separates a re-run from earlier results (run names, jsonl, summary).
TAG = os.environ.get("MONDAY_TAG", "mon")
STEM = "monday" if TAG == "mon" else f"monday_{TAG}"
OUT = ROOT / "results" / f"{STEM}.jsonl"
spikelib.BASE_ENV = {"QSG_RHI_BACKEND": "d3d12", "SPIKE_PACED": 1}

FIXES = {0: "plain", 1: "settling (Qt docs)", 2: "row-owned animations", 3: "time-bound opacity", 4: "motion only"}
BURSTS = ["burst:2,30,1000", "burst:3,30,1000", "burst:5,15,1000", "burst:3,0,1000"]
RATES = ["5", "20", "50", "100", "200"]


def log(record):
    with open(OUT, "a") as f:
        print(json.dumps({"time": time.strftime("%H:%M:%S"), **record}), file=f)
    print(json.dumps(record), flush=True)


def step_queue():
    clean = []
    for fix in FIXES:
        broken = 0
        for load in BURSTS + RATES:
            secs = 60 if load.startswith("burst") else 8
            name = f"{TAG}_q{fix}_{load.replace(':', '_').replace(',', '_')}"
            # A capture that missed a desktop frame cannot certify a clean
            # result, so it is repeated (up to three attempts).
            for attempt in range(3):
                d, out = queue_diag.main(f"{name}_a{attempt}", load, fix, secs, snaps_after=(600, 2000))
                if out["problems"] > 0 or out["recorder_missed"] == 0:
                    break
            log({"step": "queue", "fix": fix, "variant": FIXES[fix], "attempts": attempt + 1, **out})
            broken += not out["certified_clean"]
        if broken == 0:
            clean.append(fix)
    # Full scenario (drags, core edits, keyboard bursts), PresentMon and
    # duplication pass, for the clean variants and Qt's documented one.
    for fix in sorted(set(clean) | {1}):
        for rate in (5, 20, 100):
            pm, qc = Q.run_rate(f"{TAG}f{fix}", rate, fix)
            log({"step": "queue_scenario", "fix": fix, "rate": rate, "dropped_per_min": pm["dropped_per_min"],
                 "problems": qc["problems"], "frames_with_problems": qc["frames_with_problems"],
                 "persistent_at_end": qc["persistent_at_end"], "recorder_missed": qc["recorder_missed"],
                 "certified_clean": qc["certified_clean"]})


def step_motion():
    # The full scenario for the motion-only variant (4), added after the
    # 2026-10-19 re-run: it was clean at every load but its burst captures
    # missed desktop frames, so step_queue did not pick it.
    for rate in (5, 20, 100):
        for attempt in range(3):
            pm, qc = Q.run_rate(f"{TAG}f4a{attempt}", rate, 4)
            if qc["problems"] > 0 or qc["recorder_missed"] == 0:
                break
        log({"step": "queue_scenario", "fix": 4, "rate": rate, "attempts": attempt + 1,
             "dropped_per_min": pm["dropped_per_min"], "problems": qc["problems"],
             "frames_with_problems": qc["frames_with_problems"], "persistent_at_end": qc["persistent_at_end"],
             "recorder_missed": qc["recorder_missed"], "certified_clean": qc["certified_clean"]})


def step_rates():
    # The churn axis with true rates (the 100/s step ran at 125/s before).
    ramps.run_axis(TAG, "churn")


def page_times(d, keys=("key 49 down", "key 50 down")):
    """Per page switch: time from the key to the Loader's onLoaded mark (ring)
    and to the last change of the page region within 900 ms (duplication)."""
    ins = [(q, c) for t, q, c in analyze.inputs(d) if c.startswith(keys)]
    rg = analyze.ring(d)
    loaded = [q for q, k, a, t in rg["events"] if k == 16 and a >= 100] if rg else []
    out = []
    for q, c in ins:
        l = next((x for x in loaded if x > q), None)
        out.append(round((l - q) / 1e4, 1) if l else None)
    return out


def last_change(d, region, keys=("key 49 down", "key 50 down"), horizon_ms=900):
    _, cols, rows = analyze.dupcap(d)
    res = []
    for t, q, c in analyze.inputs(d):
        if not c.startswith(keys):
            continue
        prev, last = None, None
        for r in rows:
            if r["present_qpc"] <= q:
                prev = r[region]
                continue
            if r["present_qpc"] > q + horizon_ms * 1e4:
                break
            if r[region] != prev:
                last = r["present_qpc"]
            prev = r[region]
        res.append(round((last - q) / 1e4, 1) if last else None)
    return res


def step_pages():
    for weight in (1, 2):
        for asyn in (0, 1):
            cfg = {"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": weight, "SPIKE_FRESH_PAGES": 1, "SPIKE_ASYNC_PAGES": asyn}
            name = f"{TAG}_pages_w{weight}_async{asyn}"
            pm = measure(name + "_pm", cfg, ui_scenarios.pages_script(),
                         extra={"step": "pages", "scenario": "pages_incubation"})
            d = run(name + "_dd", cfg, ui_scenarios.pages_script(), mode="dd",
                    rects={"target": ui_scenarios.PAGE_RECT})
            lat = analyze.input_to_photon(d, "target", lambda c: c.startswith(("key 49 down", "key 50 down")))
            loaded = [x for x in page_times(d) if x is not None]
            settled = [x for x in last_change(d, "target") if x is not None]
            log({"step": "pages", "weight": weight, "async": asyn, "dropped_per_min": pm["dropped_per_min"],
                 "max_frame_ms": pm["max_ms"], "first_change_p50_ms": lat["p50_ms"],
                 "loaded_ms_p50": analyze.pct(loaded, 50), "loaded_ms_max": max(loaded, default=None),
                 "populated_ms_p50": analyze.pct(settled, 50), "populated_ms_max": max(settled, default=None)})


def step_soak(minutes=180):
    k = int(display()["scale"]) / 100
    name = f"{TAG}_soak{minutes}"
    cfg = {"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": 1, "SPIKE_QUEUE_OPS": 5, "SPIKE_QUEUE_FIX": best_fix(),
           "SPIKE_RING": 1 << 22, "SPIKE_MEM_OUT": RUNS / name / "memory.csv",
           "QT_LOGGING_RULES": "qt.qml.gc.statistics=true"}
    (RUNS / name).mkdir(parents=True, exist_ok=True)
    d = run(name, cfg, misc_scenarios.soak_script(minutes, k), mode="pm", tail=2)
    log({"step": "soak", "minutes": minutes, **soak_summary(d)})


def best_fix():
    """The first variant with no problems in today's queue runs, else 1."""
    if not OUT.exists():
        return 1
    rows = [json.loads(l) for l in open(OUT)]
    for fix in (4, 3, 2, 1):
        rs = [r for r in rows if r.get("step") == "queue" and r.get("fix") == fix]
        if rs and all(r["problems"] == 0 for r in rs):
            return fix
    return 1


def soak_summary(d):
    out = {}
    mem = d / "memory.csv"
    if mem.exists():
        rows = [list(map(int, l.split(","))) for l in open(mem) if l[0].isdigit()]
        q0 = rows[0][0]
        series = [((r[0] - q0) / 6e8, r[1] / 2**20, r[2] / 2**20, r[3] / 2**20, r[4] / 2**20) for r in rows]
        def at(t0, t1, i):
            v = [s[i] for s in series if t0 <= s[0] < t1]
            return round(statistics.median(v), 1) if v else None
        end = series[-1][0]
        for i, label in ((1, "private_mb"), (2, "heap_committed_mb"), (3, "heap_allocated_mb"), (4, "gpu_local_mb")):
            out[label] = [at(m, m + 15, i) for m in range(0, int(end) + 1, 15)]
        warm = [s for s in series if s[0] >= 30]
        if len(warm) > 2:
            n = len(warm)
            mt = sum(s[0] for s in warm) / n
            for i, label in ((1, "private"), (3, "heap_allocated"), (4, "gpu_local")):
                mv = sum(s[i] for s in warm) / n
                slope = sum((s[0] - mt) * (s[i] - mv) for s in warm) / sum((s[0] - mt) ** 2 for s in warm)
                out[f"{label}_slope_mb_per_hour_after_30min"] = round(slope * 60, 2)
    frames = analyze.presentmon(d)
    if frames:
        hz = analyze.refresh_hz(analyze.meta(d))
        T = 1000 / hz
        per_15 = {}
        for f in frames[1:]:
            if f["MsBetweenPresents"] > 1.5 * T:
                b = int((f["qpc"] - frames[0]["qpc"]) / 9e9)
                per_15[b] = per_15.get(b, 0) + round(f["MsBetweenPresents"] / T) - 1
        out["dropped_per_15min"] = [per_15.get(i, 0) for i in range(int((frames[-1]["qpc"] - frames[0]["qpc"]) / 9e9) + 1)]
    log_file = d / "app.log"
    if log_file.exists():
        gc = [l for l in open(log_file, errors="replace") if "qt.qml.gc" in l]
        out["gc_log_lines"] = len(gc)
        out["gc_first"] = gc[:3]
        out["gc_last"] = gc[-3:]
    return out


def summary_md():
    rows = [json.loads(l) for l in open(OUT)] if OUT.exists() else []
    lines = ["# Monday batch summary (generated)", ""]
    q = [r for r in rows if r["step"] == "queue"]
    if q:
        lines += ["## Queue variants", "", "| variant | load | measured rate | frames with problems | still broken at end | recorder missed | certified clean |",
                  "|---|---|---|---|---|---|---|"]
        lines += [f"| {r['fix']} {r['variant']} | {r['load']} | {r['measured_rate']} | {r.get('frames_with_problems')} {r['by_kind']} | "
                  f"{r['persistent_at_end']} | {r.get('recorder_missed')} | {r.get('certified_clean')} |" for r in q]
    for step in ("queue_scenario", "pages", "soak"):
        rs = [r for r in rows if r["step"] == step]
        if rs:
            lines += ["", f"## {step}", ""] + ["- " + json.dumps(r) for r in rs]
    (ROOT / "results" / f"{STEM}_summary.md").write_text("\n".join(lines) + "\n")


def archive():
    subprocess.run(["7z", "a", "-t7z", "-mx=9", "-bso0", "-bsp0", str(ROOT / "results" / "raw" / f"{STEM}.7z"),
                    f"{TAG}*"], cwd=RUNS)


STEPS = {"queue": step_queue, "motion": step_motion, "rates": step_rates, "pages": step_pages, "soak": step_soak}

if __name__ == "__main__":
    original = display()
    print("original display", original, flush=True)
    heavy = spikelib.heavy_processes()
    if heavy:
        sys.exit(f"heavy processes running, close them first: {heavy}")
    try:
        set_condition(144, 100)
        for name in sys.argv[1:] or list(STEPS):
            print("==", name, flush=True)
            try:
                STEPS[name]()
            except Exception:
                traceback.print_exc()
                log({"step": name, "error": traceback.format_exc()[-500:]})
    finally:
        harness("display", "refresh", original["refresh"])
        harness("display", "scale", original["scale"])
        print("restored display", display(), flush=True)
        summary_md()
        archive()
        print("done; results in results/monday.jsonl and results/monday_summary.md", flush=True)
