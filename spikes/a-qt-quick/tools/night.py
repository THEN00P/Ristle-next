"""Remaining runs after the matrix: 60 Hz control rerun, 144 Hz diagnosis,
idle memory and the 30-minute soak. Restores 59.951 Hz / 100 % at the end
(the user's setting before the spike)."""
import time, traceback
import condition as C, diag144, misc_scenarios
from matrix import set_condition
from spikelib import harness, display


def step(name, f):
    print(f"== {name}", flush=True)
    try:
        f()
    except Exception:
        traceback.print_exc()
        print(f"!! {name} failed", flush=True)


try:
    set_condition(59, 100)
    time.sleep(3)
    step("c59s100 control", lambda: C.control("c59s100"))
    step("c59s100 idle", lambda: misc_scenarios.idle("c59s100"))
    set_condition(144, 100)
    step("c144s100 diag", lambda: diag144.main("c144s100"))
    step("c144s100 idle", lambda: misc_scenarios.idle("c144s100"))
    step("c144s100 soak", lambda: misc_scenarios.soak("c144s100", 30))
finally:
    harness("display", "refresh", "59")
    harness("display", "scale", "100")
    print("restored display", display(), flush=True)
