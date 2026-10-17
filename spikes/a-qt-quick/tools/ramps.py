"""Load ramps for the track table (scenario 2), one axis at a time and then
combined, at the current display condition. Usage: ramps.py <condition-tag> [axis...]"""
import subprocess, sys
from scenarios import measure, scroll_script, short

BASE = {"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": 1, "SPIKE_ART_CACHE": 1, "SPIKE_ART_THREADS": 4}
FLING = 8000
POWER_SAVER = "a1841308-3541-4fab-bc81-f71556f20b4a"
HIGH_PERF = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"
WARP = {"QSG_RHI_BACKEND": "d3d11", "QSG_RHI_PREFER_SOFTWARE_RENDERER": "1"}
LLVMPIPE = {"QSG_RHI_BACKEND": "opengl", "QT_OPENGL": "software"}
IGPU = {"QSG_RHI_BACKEND": "d3d11", "QT_D3D_ADAPTER_INDEX": "1"}

AXES = {
    "rows": [({**BASE, "SPIKE_ROWS": n}, ("fling", FLING), None) for n in (1000, 10000, 100000, 1000000)],
    "weight": [({**BASE, "SPIKE_WEIGHT": w}, ("fling", FLING), None) for w in (0, 1, 3, 2)],
    "churn": [({**BASE, "SPIKE_CHURN": c}, ("fling", FLING), None) for c in (1, 10, 100, 1000)],
    "fling": [(BASE, ("fling", v), None) for v in (1000, 2000, 4000, 8000, 12000)],
    "wheel": [(BASE, ("wheel", n), None) for n in (5, 10, 20, 40)],
    "touchpad": [(BASE, ("touchpad", v), None) for v in (1000, 4000, 10000, 20000)],
    "artwork": [({**BASE, "SPIKE_ART_CACHE": 1, "SPIKE_ART_THREADS": 4}, ("fling", FLING), None)] +
               [({**BASE, "SPIKE_ART_CACHE": 0, "SPIKE_ART_THREADS": t}, ("fling", FLING), None) for t in (1, 4, 16)],
    "cpu": [(BASE, ("fling", FLING), a) for a in (None, 0x5, 0x1)] +
           [(BASE, ("fling", FLING), ("powersaver", a)) for a in (None, 0x1)],
    "gpu": [({**BASE, **g}, ("fling", FLING), None) for g in ({}, IGPU, WARP)],
    # Software rendering pass criterion: 1,000 rows with artwork, no drops.
    "warp_production": [({**BASE, **WARP, "SPIKE_ROWS": 1000}, (kind, v), None)
                        for kind, v in (("fling", 2000), ("fling", FLING), ("wheel", 10), ("touchpad", 4000))],
    "llvmpipe_production": [({**BASE, **LLVMPIPE, "SPIKE_ROWS": 1000}, (kind, v), None)
                            for kind, v in (("fling", 2000), ("fling", FLING), ("wheel", 10), ("touchpad", 4000))],
    "llvmpipe_sw2d_production": [({**BASE, "QT_QUICK_BACKEND": "software", "SPIKE_GLOW": 0, "SPIKE_ROWS": 1000}, (kind, v), None)
                                 for kind, v in (("fling", 2000), ("fling", FLING), ("wheel", 10), ("touchpad", 4000))],
    "llvmpipe_hw_control": [({**BASE, "SPIKE_ROWS": 1000}, (kind, v), None)
                            for kind, v in (("fling", 2000), ("fling", FLING), ("wheel", 10), ("touchpad", 4000))],
    "llvmpipe_load": [({**BASE, **LLVMPIPE, "SPIKE_ROWS": n, "SPIKE_WEIGHT": w}, ("fling", 12000), a)
                      for n, w, a in ((1000000, 1, None), (1000000, 2, None), (1000000, 1, 0x5), (1000000, 1, 0x1))],
    # Combined: escalate until frames drop.
    "combined": [
        ({**BASE, "SPIKE_ROWS": 1000000, "SPIKE_WEIGHT": 2, "SPIKE_CHURN": 100}, ("fling", 12000), None),
        ({**BASE, "SPIKE_ROWS": 1000000, "SPIKE_WEIGHT": 2, "SPIKE_CHURN": 100, "SPIKE_ART_CACHE": 0}, ("fling", 12000), 0x1),
        ({**BASE, **WARP, "SPIKE_ROWS": 1000000, "SPIKE_WEIGHT": 2, "SPIKE_CHURN": 100}, ("fling", 12000), None),
        ({**BASE, **WARP, "SPIKE_ROWS": 1000000, "SPIKE_WEIGHT": 2, "SPIKE_CHURN": 1000, "SPIKE_ART_CACHE": 0}, ("fling", 12000), 0x1),
    ],
}


def powercfg(guid):
    subprocess.run(["powercfg", "/setactive", guid], check=True)


def run_axis(tag, axis):
    results = []
    for i, (cfg, (kind, speed), cpu) in enumerate(AXES[axis]):
        affinity, saver = cpu, False
        if isinstance(cpu, tuple):
            saver, affinity = True, cpu[1]
        name = f"{tag}_{axis}_{i}"
        if saver:
            powercfg(POWER_SAVER)
        try:
            # Mesa llvmpipe presents through GDI: no DXGI presents for
            # PresentMon, so those axes use the duplication pass.
            mode = "dd" if axis.startswith("llvmpipe") else "pm"
            r = measure(name, cfg, scroll_script(kind, speed), affinity=affinity, mode=mode,
                        rects={"list": (248, 200, 680, 480)} if mode == "dd" else None,
                        extra={"axis": axis, "step": i, "input": f"{kind}:{speed}",
                               "affinity": affinity, "power_saver": saver})
        finally:
            if saver:
                powercfg(HIGH_PERF)
        print(axis, i, kind, speed, affinity, saver, short(r), flush=True)
        results.append(r)
    return results


if __name__ == "__main__":
    tag = sys.argv[1]
    for axis in sys.argv[2:] or list(AXES):
        run_axis(tag, axis)
