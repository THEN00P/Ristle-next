"""Builds the report tables from results/*.jsonl."""
import json, sys
from collections import defaultdict
from spikelib import ROOT

R = ROOT / "results"


def load(name):
    p = R / name
    return [json.loads(l) for l in open(p)] if p.exists() else []


def ramp_tables(tag_filter=None):
    rows = [r for r in load("summary.jsonl") if r.get("axis") and r["mode"] == "pm"]
    by = defaultdict(list)
    for r in rows:
        tag = r["run"].split("_")[0]
        if tag_filter and tag != tag_filter:
            continue
        by[(tag, r["axis"])].append(r)
    out = []
    for (tag, axis), rs in sorted(by.items()):
        rs.sort(key=lambda r: r["step"])
        knee = next((r for r in rs if r["dropped"] > 0), None)
        out.append(f"\n### {tag} · {axis}  (knee: {describe(knee) if knee else 'no drops'})\n")
        out.append("| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p99 | ring GUI / render excess |")
        out.append("|---|---|---|---|---|---|---|---|---|")
        for r in rs:
            out.append(f"| {r['step']} | {describe(r)} | {r['frames']}/{r['expected_frames']} | {fmt(r['p50_ms'])} | "
                       f"{fmt(r['p99_ms'])} | {fmt(r['max_ms'])} | {r['dropped_per_min']} | {fmt(r['gpu_busy_p99_ms'])} | "
                       f"{fmt(r.get('ring_gui_excess_max_ms'))} / {fmt(r.get('ring_render_excess_max_ms'))} |")
    return "\n".join(out)


def describe(r):
    c = r["config"]
    parts = []
    for k, label in (("SPIKE_ROWS", "rows"), ("SPIKE_WEIGHT", "weight"), ("SPIKE_CHURN", "churn/s"),
                     ("SPIKE_QUEUE_OPS", "queue ops/s"), ("SPIKE_GLOW", "glow"), ("SPIKE_FRESH_PAGES", "fresh")):
        if k in c:
            v = c[k]
            # Before the pacing fix (runs without SPIKE_PACED) rates of 60/s and
            # more ran as round(rate * 0.016) edits per 16 ms tick.
            if k in ("SPIKE_CHURN", "SPIKE_QUEUE_OPS") and int(v) >= 60 and not c.get("SPIKE_PACED"):
                actual = round(round(int(v) * 0.016) / 0.016)
                parts.append(f"{label} {v}" + (f" (ran {actual})" if actual != int(v) else ""))
            else:
                parts.append(f"{label} {v}")
    if c.get("SPIKE_ART_CACHE") == 0:
        parts.append(f"uncached art ×{c.get('SPIKE_ART_THREADS', 4)}")
    if c.get("QSG_RHI_PREFER_SOFTWARE_RENDERER"):
        parts.append("WARP")
    if c.get("QT_D3D_ADAPTER_INDEX") == "1":
        parts.append("iGPU")
    if r.get("affinity"):
        parts.append(f"affinity {hex(r['affinity'])}")
    if r.get("power_saver"):
        parts.append("power saver")
    if r.get("input"):
        parts.append(r["input"])
    return ", ".join(parts)


def fmt(v):
    return "–" if v is None else f"{v:.1f}"


if __name__ == "__main__":
    print(ramp_tables(sys.argv[1] if len(sys.argv) > 1 else None))
