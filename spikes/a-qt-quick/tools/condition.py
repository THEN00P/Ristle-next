"""Runs a set of scenarios at the current display condition.
Usage: condition.py <tag> control|queue|ui|glow|text|ramps|core..."""
import json, sys
import analyze
from control import script as control_script
from spikelib import display, run
from scenarios import SUMMARY

SETS = {
    "core": ["control", "ramps", "queue", "ui", "glow", "text"],
}


def control(tag):
    k = int(display()["scale"]) / 100
    for mode in ("animation", "scroll"):
        for pass_ in ("pm", "dd"):
            d = run(f"{tag}_control_{mode}_{pass_}", {"SPIKE_ROWS": 1000, "SPIKE_WEIGHT": 1},
                    control_script(k, mode == "scroll"), mode=pass_,
                    rects={"list": (248, 200, 680, 480)})
            r = (analyze.control if pass_ == "pm" else analyze.control_dd)(d)
            r.update({"tag": tag, "during": mode, "pass": pass_})
            with open(SUMMARY.parent / "control.jsonl", "a") as f:
                print(json.dumps(r), file=f)
            ok = sum(s["detected"] for s in r["stalls"])
            print("control", mode, pass_, f"{ok}/{len(r['stalls'])} detected",
                  [(s["thread"][0], s["injected_ms"], s["measured_ms"]) for s in r["stalls"]], flush=True)


def ramps(tag):
    import ramps as R
    for axis in [a for a in R.AXES if not a.startswith("llvmpipe")] + ["llvmpipe_production", "llvmpipe_hw_control"]:
        R.run_axis(tag, axis)


def queue(tag):
    import queue_scenario
    for rate in (1, 5, 20, 50, 100, 200):
        queue_scenario.run_rate(tag, rate)


def ui(tag):
    import ui_scenarios
    ui_scenarios.run_steps(tag, "sidebar")
    ui_scenarios.run_steps(tag, "pages")


def glow(tag):
    import misc_scenarios
    misc_scenarios.glow(tag)


def text(tag):
    import misc_scenarios
    misc_scenarios.text(tag)


if __name__ == "__main__":
    tag = sys.argv[1]
    for item in sys.argv[2:]:
        for step in SETS.get(item, [item]):
            print("==", step, flush=True)
            globals()[step](tag)
