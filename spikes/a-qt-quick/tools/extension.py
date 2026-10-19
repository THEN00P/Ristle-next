"""Rule 4 follow-up: on D3D12 at 144 Hz the CPU-side axes did not break
within their specified ramps. Push them further until frames drop, then run
a 60-minute soak to settle the memory trend. Restores 59.951 Hz / 100 %."""
import traceback
import matrix as M, misc_scenarios, ramps, spikelib
from spikelib import harness, display

spikelib.BASE_ENV = {"QSG_RHI_BACKEND": "d3d12"}
B = ramps.BASE
ramps.AXES.update({
    "ext_churn": [({**B, "SPIKE_CHURN": c}, ("fling", 8000), None) for c in (3000, 10000, 30000)],
    "ext_rows": [({**B, "SPIKE_ROWS": n}, ("fling", 12000), None) for n in (10_000_000, 40_000_000)],
    "ext_cpu": [({**B, "SPIKE_WEIGHT": 2}, ("fling", 12000), a) for a in (0x1, ("powersaver", 0x1))] +
               [({**B, "SPIKE_WEIGHT": 2, "SPIKE_CHURN": c}, ("fling", 12000), ("powersaver", 0x1)) for c in (100, 1000)],
    "ext_combined": [({**B, "SPIKE_ROWS": 10_000_000, "SPIKE_WEIGHT": 2, "SPIKE_CHURN": c, "SPIKE_ART_CACHE": 0},
                      ("fling", 12000), ("powersaver", 0x1)) for c in (1000, 3000, 10000)],
})

try:
    M.set_condition(144, 100)
    for axis in ("ext_churn", "ext_rows", "ext_cpu", "ext_combined"):
        print("==", axis, flush=True)
        try:
            ramps.run_axis("d12c144s100", axis)
        except Exception:
            traceback.print_exc()
            print("!!", axis, flush=True)
    print("== soak60", flush=True)
    try:
        misc_scenarios.soak("d12c144s100long", 60)
    except Exception:
        traceback.print_exc()
        print("!! soak60", flush=True)
finally:
    harness("display", "refresh", "59")
    harness("display", "scale", "100")
    print("restored display", display(), flush=True)
