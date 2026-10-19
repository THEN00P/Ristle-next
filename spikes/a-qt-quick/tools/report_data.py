"""Generates the data sections of docs/research/qt-quick-spike.md from
results/*.jsonl. Prints markdown; writes the long per-axis tables to
results/ramps.md. Usage: report_data.py > results/tables.md"""
import json
from collections import defaultdict
from spikelib import ROOT
from tables import describe, fmt

R = ROOT / "results"
ORDER = ["c144s100", "c59s100", "c144s125", "c59s125", "c144s150", "c59s150",
         "d12c144s100", "d12c59s100", "d12c144s125", "d12c144s150"]


def load(name):
    p = R / name
    rows = [json.loads(l) for l in open(p)] if p.exists() else []
    if name == "summary.jsonl":
        # Display drops (MsBetweenDisplayChange) next to present drops, from
        # tools/display_drops.py (Codex review, final round).
        disp = {r["run"]: r for r in load("display_drops.jsonl")}
        for r in rows:
            r.update({k: v for k, v in disp.get(r["run"], {}).items()
                      if k in ("display_dropped", "display_dropped_per_min", "display_max_ms")})
    return rows


def cond_label(tag):
    d12 = tag.startswith("d12")
    t = tag[3:] if d12 else tag
    hz = "144 Hz" if t.startswith("c144") else "60 Hz"
    return f"{hz} / {t.split('s')[-1]} %" + (" · D3D12" if d12 else "")


def tag_of(run):
    return run.split("_")[0]


def conditions():
    rows = load("summary.jsonl")
    by = defaultdict(list)
    for r in rows:
        by[tag_of(r["run"])].append(r)
    out = ["| condition | refresh (exact) | runs | background CPU % (min–max) | power plan |", "|---|---|---|---|---|"]
    for t in ORDER:
        rs = by.get(t)
        if not rs:
            continue
        bg = [r["background_cpu_pct"] for r in rs]
        out.append(f"| {cond_label(t)} | {rs[0]['refresh_hz']:.3f} Hz | {len(rs)} | {min(bg):.1f}–{max(bg):.1f} | "
                   f"{', '.join(sorted({r['power_plan'] for r in rs}))} |")
    return "\n".join(out)


def control():
    rows = load("control.jsonl")
    out = ["| condition | during | pass | detected | injected → measured (ms), GUI thread | render thread |",
           "|---|---|---|---|---|---|"]
    for t in ORDER:
        for r in [x for x in rows if x["tag"] == t]:
            ok = sum(s["detected"] for s in r["stalls"])
            g = ", ".join(f"{s['injected_ms']}→{s['measured_ms']:.1f}" for s in r["stalls"] if s["thread"] == "gui")
            rd = ", ".join(f"{s['injected_ms']}→{s['measured_ms']:.1f}" for s in r["stalls"] if s["thread"] == "render")
            out.append(f"| {cond_label(t)} | {r['during']} | {'PresentMon' if r['pass'] == 'pm' else 'duplication'} | "
                       f"{ok}/{len(r['stalls'])} | {g} | {rd} |")
    return "\n".join(out)


def knees():
    rows = [r for r in load("summary.jsonl") if r.get("axis") and r.get("scenario") is None]
    by = defaultdict(list)
    for r in rows:
        by[(tag_of(r["run"]), r["axis"])].append(r)
    axes = sorted({a for _, a in by})
    out = ["| axis | " + " | ".join(cond_label(t) for t in ORDER if any((t, a) in by for a in axes)) + " |"]
    tags = [t for t in ORDER if any((t, a) in by for a in axes)]
    out.append("|---|" + "---|" * len(tags))
    for a in axes:
        cells = []
        for t in tags:
            rs = sorted(by.get((t, a), []), key=lambda r: r["step"])
            if not rs:
                cells.append("–")
                continue
            k = next((r for r in rs if dropped(r) > 0), None)
            kd = next((r for r in rs if dropped(r, display=True) > 0), None)
            cell = f"**{describe(k)}** ({dropped_per_min(k)}/min)" if k else "no drops"
            if rs[0].get("mode") != "dd":
                cell += f"; display: **{describe(kd)}** ({kd['display_dropped_per_min']}/min)" if kd else "; display: no drops"
            cells.append(cell)
        out.append(f"| {a} | " + " | ".join(cells) + " |")
    return "\n".join(out)


def dropped(r, display=False):
    if r.get("mode") == "dd":
        return r["gui_clock_skipped"] + r["cadence"]["skipped_refreshes"]
    return r.get("display_dropped", 0) if display else r["dropped"]


def dropped_per_min(r):
    if r.get("mode") == "dd":
        return r["cadence"]["skipped_per_min"]
    return r["dropped_per_min"]


def ramp_details():
    rows = [r for r in load("summary.jsonl") if r.get("axis") and r.get("scenario") is None]
    by = defaultdict(list)
    for r in rows:
        by[(tag_of(r["run"]), r["axis"])].append(r)
    out = []
    for t in ORDER:
        for a in sorted({a for tt, a in by if tt == t}):
            rs = sorted(by[(t, a)], key=lambda r: r["step"])
            out.append(f"\n#### {cond_label(t)} · {a}\n")
            if rs[0].get("mode") == "dd":
                out.append("| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |")
                out.append("|---|---|---|---|---|---|---|")
                for r in rs:
                    c = r["cadence"]
                    out.append(f"| {r['step']} | {describe(r)} | {c['updates_per_s']} | {c['skipped_refreshes']} "
                               f"({c['skipped_per_min']}) | {c['longest_freeze_ms']} | {r['gui_clock_skipped']} | {fmt(r['gui_clock_max_ms'])} |")
                continue
            out.append("| step | load | frames/expected | p50 | p99 | max | dropped/min presents (display) | display max | GPU busy p50/p99 | ring GUI/render excess |")
            out.append("|---|---|---|---|---|---|---|---|---|---|")
            for r in rs:
                out.append(f"| {r['step']} | {describe(r)} | {r['frames']}/{r['expected_frames']} | {fmt(r['p50_ms'])} | "
                           f"{fmt(r['p99_ms'])} | {fmt(r['max_ms'])} | {r['dropped_per_min']} ({r.get('display_dropped_per_min', '–')}) | "
                           f"{fmt(r.get('display_max_ms'))} | {fmt(r['gpu_busy_p50_ms'])}/{fmt(r['gpu_busy_p99_ms'])} | "
                           f"{fmt(r.get('ring_gui_excess_max_ms'))}/{fmt(r.get('ring_render_excess_max_ms'))} |")
    return "\n".join(out)


def queue():
    qc = load("queue_checks.jsonl")
    pm = {r["run"]: r for r in load("summary.jsonl") if r.get("scenario") == "queue"}
    out = ["| condition | transitions | edits/s configured (measured) | PresentMon dropped/min presents [display] (p99, max ms) | frames checked (settled) | corruption frames | still broken after edits stop | insert/remove input-to-photon p50/p99 ms |",
           "|---|---|---|---|---|---|---|---|"]
    for t in ORDER:
        for r in sorted([x for x in qc if x["tag"] == t], key=lambda x: (not x["documented_settling"], x["rate"])):
            p = pm.get(r["run"].replace("_dd", "_pm"), {})
            lat = r.get("latency", {})
            out.append(f"| {cond_label(t)} | {'settling' if r['documented_settling'] else 'plain'} | {r['rate']} ({measured_rate(r['run'])}) | "
                       f"{p.get('dropped_per_min', '–')} [{p.get('display_dropped_per_min', '–')}] ({fmt(p.get('p99_ms'))}, {fmt(p.get('max_ms'))}) | "
                       f"{r['frames']} ({r['settled_frames']}) | {r['problems']} {kinds(r['by_kind'])} | {r['persistent_at_end']} | "
                       f"{fmt(lat.get('p50_ms'))}/{fmt(lat.get('p99_ms'))} |")
    return "\n".join(out)


def measured_rate(run):
    """Edits per second the core actually applied between F2 on and F2 off
    (from the ring buffer). Runs before the 2026-10-18 pacing fix batched
    edits per 16 ms tick, so 100 and 200 ran faster than labelled."""
    import analyze
    from spikelib import RUNS
    d = RUNS / run
    if not (d / "ring.csv").exists():
        return "–"
    ins = analyze.inputs(d)
    f2 = [q for t, q, c in ins if c == "key 113 down"]
    if len(f2) < 2:
        return "–"
    ev = [q for q, k, a, t in analyze.ring(d)["events"] if k in (13, 14, 15) and f2[0] <= q <= f2[1]]
    return round(len(ev) / ((f2[1] - f2[0]) / 1e7))


def kinds(k):
    return "(" + ", ".join(f"{a} {b}" for a, b in k.items()) + ")" if k else ""


def ui():
    lat = load("latency.jsonl")
    pm = {r["run"]: r for r in load("summary.jsonl") if r.get("scenario") in ("sidebar", "pages")}
    out = ["| condition | scenario | load | PresentMon dropped/min presents [display] | p99 / max frame ms | input-to-photon p50 / p99 ms |",
           "|---|---|---|---|---|---|"]
    for t in ORDER:
        for l in [x for x in lat if x["tag"] == t]:
            p = pm.get(f"{t}_{l['scenario']}_{l['step']}_pm", {})
            out.append(f"| {cond_label(t)} | {l['scenario']} | {describe(p) if p else ''} | {p.get('dropped_per_min', '–')} [{p.get('display_dropped_per_min', '–')}] | "
                       f"{fmt(p.get('p99_ms'))} / {fmt(p.get('max_ms'))} | {fmt(l['p50_ms'])} / {fmt(l['p99_ms'])} |")
    return "\n".join(out)


def glow():
    rows = [r for r in load("summary.jsonl") if r.get("scenario") == "glow"]
    out = ["| condition | load | GPU busy p50 / p99 ms | frame p99 / max ms | dropped/min presents [display] |", "|---|---|---|---|---|"]
    for t in ORDER:
        for r in sorted([x for x in rows if tag_of(x["run"]) == t], key=lambda x: x["step"]):
            out.append(f"| {cond_label(t)} | {describe(r)} | {fmt(r['gpu_busy_p50_ms'])} / {fmt(r['gpu_busy_p99_ms'])} | "
                       f"{fmt(r['p99_ms'])} / {fmt(r['max_ms'])} | {r['dropped_per_min']} [{r.get('display_dropped_per_min', '–')}] |")
    return "\n".join(out)


def text():
    rows = load("text.jsonl")
    out = ["| scale | renderer | acutance (dark / white band) | solid ink (dark / white) | colour fringing (white band) | line width px (12/13/14) |",
           "|---|---|---|---|---|---|"]
    by = defaultdict(list)
    for r in rows:
        by[(r["scale"], r["renderer"])].append(r)
    for (scale, rend), rs in sorted(by.items()):
        d = [r for r in rs if r["band"] == "dark"]
        w = [r for r in rs if r["band"] == "white"]
        avg = lambda xs, k: sum(x[k] for x in xs) / max(1, len(xs))
        out.append(f"| {round(scale * 100)} % | {rend} | {avg(d, 'acutance'):.3f} / {avg(w, 'acutance'):.3f} | "
                   f"{avg(d, 'solid'):.2f} / {avg(w, 'solid'):.2f} | {avg(w, 'chroma'):.0f} | "
                   f"{'/'.join(str(x['width']) for x in sorted(w, key=lambda x: x['px']))} |")
    return "\n".join(out)


def baselines():
    rows = load("baselines.jsonl")
    out = ["| condition | app | input | content updates/s while moving | skipped refreshes per min moving | longest freeze ms | PresentMon target | frames/expected, dropped |",
           "|---|---|---|---|---|---|---|---|"]
    for t in ORDER:
        for r in [x for x in rows if x["tag"] == t]:
            c, p = r["cadence"], r["presentmon"]
            out.append(f"| {cond_label(t)} | {r['app']} | {r['input']} | {c['updates_per_s']} | "
                       f"{c['skipped_per_min']} | {c['longest_freeze_ms']} | {r['presentmon_target']} | {p['frames']}/{p['expected_frames']}, {p['dropped']} |")
    return "\n".join(out)


def startup():
    out = []
    for r in load("startup.jsonl"):
        out.append(f"{cond_label(r['tag'])}: first present after launch, ms: " +
                   ", ".join(str(x["first_present_ms"]) for x in r["launches"]))
    return "\n".join(out)


def soak_section():
    import csv, statistics
    from spikelib import RUNS
    import analyze
    out = []
    for tag in ("c144s100", "d12c144s100", "d12c144s100long"):
        d = RUNS / f"{tag}_soak"
        if not (d / "presentmon.csv").exists():
            continue
        m = analyze.meta(d)
        hz = analyze.refresh_hz(m)
        frames = analyze.presentmon(d)
        t0 = frames[0]["qpc"]
        per_min, disp_min = defaultdict(int), defaultdict(int)
        T = 1000 / hz
        for f in frames[1:]:
            if f["MsBetweenPresents"] > 1.5 * T:
                per_min[int((f["qpc"] - t0) / 6e8)] += round(f["MsBetweenPresents"] / T) - 1
        shown = [f for f in frames if f["MsUntilDisplayed"]]
        for f in shown[1:]:
            x = f["MsBetweenDisplayChange"]
            if x and x > 1.5 * T:
                disp_min[int((f["qpc"] - t0) / 6e8)] += round(x / T) - 1
        mins = int((frames[-1]["qpc"] - t0) / 6e8) + 1
        drops = [per_min.get(i, 0) for i in range(mins)]
        disp = [disp_min.get(i, 0) for i in range(mins)]
        rows = list(csv.DictReader(open(d / "proc.csv")))
        q0 = int(rows[0]["qpc"])
        mem = [((int(r["qpc"]) - q0) / 6e8, float(r["private_mb"])) for r in rows]
        warm = [(t, v) for t, v in mem if t >= 5]
        first = statistics.median(v for t, v in warm if t < 10)
        last = statistics.median(v for t, v in warm if t >= max(t for t, _ in warm) - 5)
        n = len(warm)
        mt = sum(t for t, _ in warm) / n
        mv = sum(v for _, v in warm) / n
        slope = sum((t - mt) * (v - mv) for t, v in warm) / sum((t - mt) ** 2 for t, _ in warm)
        half = len(drops) // 2
        out.append(f"**{cond_label(tag.replace('d12', '').replace('long', ''))}{' (D3D12)' if tag.startswith('d12') else ' (D3D11)'}**: "
                   f"{len(frames)} presents in {mins} min; dropped frames per minute: {drops}. "
                   f"First half {sum(drops[:half])}, second half {sum(drops[half:])}. Display drops per minute "
                   f"(MsBetweenDisplayChange): {disp}, first half {sum(disp[:half])}, second half {sum(disp[half:])}. Private memory after warm-up: "
                   f"{first:.1f} MB (minutes 5–10) → {last:.1f} MB (last 5 min), slope {slope:+.3f} MB/min, "
                   f"min {min(v for _, v in warm):.1f}, max {max(v for _, v in warm):.1f} MB.")
    return "\n\n".join(out)


def memory_section():
    rows = load("memory.jsonl")
    out = ["| condition | config | private MB | working set MB | CPU % |", "|---|---|---|---|---|"]
    for r in rows:
        out.append(f"| {r['tag']} | heartbeat {'off' if r['config'].get('SPIKE_HEARTBEAT') == 0 else 'on'}, "
                   f"{r['config'].get('QSG_RHI_BACKEND', 'd3d11')} | {r['private_mb_median']:.1f} | "
                   f"{r['working_set_mb_median']:.1f} | {r['cpu_pct_median']:.2f} |")
    return "\n".join(out)


if __name__ == "__main__":
    sections = [("Conditions", conditions), ("Positive control", control), ("Knees", knees), ("Queue", queue),
                ("Sidebar and pages", ui), ("Glow", glow), ("Text", text), ("Baselines", baselines), ("Startup", startup),
                ("Idle memory", memory_section), ("Soak", soak_section)]
    for title, f in sections:
        print(f"\n## {title}\n")
        print(f())
    (R / "ramps.md").write_text("# Spike A ramp tables (generated)\n" + ramp_details() + "\n")
