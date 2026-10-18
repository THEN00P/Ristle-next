"""Second matrix pass with the D3D12 backend, the candidate production
configuration after the 144 Hz diagnosis (diag144.py). 60 Hz conditions
showed no backend-dependent drops, so this pass covers 144 Hz at all scales
plus 60 Hz / 100 % as a check. Restores 59.951 Hz / 100 % at the end."""
import time, traceback
import matrix as M, misc_scenarios, spikelib
from spikelib import harness, display

spikelib.BASE_ENV = {"QSG_RHI_BACKEND": "d3d12"}
PLAN = {
    "144:100": ["control", "ramps", "queue", "ui", "glow", "baselines", "startup"],
    "144:125": ["control", "ramps_envelope", "queue_reduced", "ui_reduced"],
    "144:150": ["control", "ramps_envelope", "queue_reduced", "ui_reduced"],
    "60:100": ["control", "ramps_envelope", "queue_reduced", "ui_reduced"],
}

try:
    for cond, steps in PLAN.items():
        hz, scale = (int(x) for x in cond.split(":"))
        d = M.set_condition(hz, scale)
        tag = f"d12c{d['refresh']}s{scale}"
        print(f"=== {cond} {tag}", flush=True)
        for step in steps:
            print(f"== {tag} {step}", flush=True)
            try:
                M.STEPS[step](tag)
            except Exception:
                traceback.print_exc()
                print(f"!! {tag} {step} failed", flush=True)
        if cond == "144:100":
            for name, f in (("idle", misc_scenarios.idle), ("soak", lambda t: misc_scenarios.soak(t, 30))):
                print(f"== {tag} {name}", flush=True)
                try:
                    f(tag)
                except Exception:
                    traceback.print_exc()
                    print(f"!! {tag} {name} failed", flush=True)
finally:
    harness("display", "refresh", "59")
    harness("display", "scale", "100")
    print("restored display", display(), flush=True)
