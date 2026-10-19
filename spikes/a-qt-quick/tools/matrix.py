"""Runs the scenario sets at every display condition (method rule 5) and
restores the original refresh rate and scaling afterwards, whatever happens.
Usage: matrix.py [condition...]   e.g. 144:100 60:125"""
import subprocess, sys, time, traceback
import condition as C
import baselines, misc_scenarios, queue_scenario, ui_scenarios, ramps
from spikelib import display, harness

FULL = ["control", "ramps", "queue", "ui", "glow", "text", "baselines", "startup"]
REDUCED = ["control", "ramps_envelope", "queue_reduced", "ui_reduced", "text", "baselines"]
CONDITIONS = {
    "60:100": FULL, "144:100": FULL,
    "60:125": REDUCED, "144:125": REDUCED, "60:150": REDUCED, "144:150": REDUCED,
}


def ramps_envelope(tag):
    for axis in ("rows", "weight", "fling", "gpu", "llvmpipe_production", "llvmpipe_hw_control", "combined"):
        ramps.run_axis(tag, axis)


def queue_reduced(tag):
    for rate in (5, 20, 50):
        queue_scenario.run_rate(tag, rate)
    queue_scenario.run_rate(tag, 5, fix=0)


def queue_full(tag):
    for rate in (1, 5, 20, 50, 100, 200):
        queue_scenario.run_rate(tag, rate)
    for rate in (1, 5, 20):
        queue_scenario.run_rate(tag, rate, fix=0)


def ui_reduced(tag):
    ui_scenarios.SIDEBAR_STEPS, sb = ui_scenarios.SIDEBAR_STEPS[1:3], ui_scenarios.SIDEBAR_STEPS
    ui_scenarios.PAGE_STEPS, pg = ui_scenarios.PAGE_STEPS[:4], ui_scenarios.PAGE_STEPS
    try:
        C.ui(tag)
    finally:
        ui_scenarios.SIDEBAR_STEPS, ui_scenarios.PAGE_STEPS = sb, pg


def run_baselines(tag):
    baselines.measure_app(tag, "qt")
    baselines.measure_app(tag, "explorer")


STEPS = {
    "control": C.control, "ramps": C.ramps, "queue": queue_full, "ui": C.ui, "glow": C.glow, "text": C.text,
    "ramps_envelope": ramps_envelope, "queue_reduced": queue_reduced, "ui_reduced": ui_reduced,
    "baselines": run_baselines, "startup": misc_scenarios.startup,
}


def set_condition(hz, scale):
    harness("display", "refresh", str(hz))
    harness("display", "scale", str(scale))
    time.sleep(4)  # let the mode change and DPI broadcast settle
    d = display()
    assert int(d["scale"]) == scale, d
    return d


if __name__ == "__main__":
    original = display()
    print("original display", original, flush=True)
    todo = sys.argv[1:] or list(CONDITIONS)
    try:
        for cond in todo:
            hz, scale = (int(x) for x in cond.split(":"))
            d = set_condition(hz, scale)
            tag = f"c{d['refresh']}s{scale}"
            print(f"=== condition {cond} -> {d}", flush=True)
            for step in CONDITIONS[cond]:
                print(f"== {tag} {step}", flush=True)
                try:
                    STEPS[step](tag)
                except Exception:
                    traceback.print_exc()
                    print(f"!! {tag} {step} failed", flush=True)
    finally:
        harness("display", "refresh", original["refresh"])
        harness("display", "scale", original["scale"])
        print("restored display", display(), flush=True)
