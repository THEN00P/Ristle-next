"""Diagnosis of the 144 Hz single-frame drops (GUI phase waiting one full
refresh) and of the Animator half-rate effect, across Qt's render-loop and
swap-chain knobs. Usage: diag144.py <tag>"""
import json, sys
from scenarios import measure, scroll_script, short, SUMMARY
from anim_rate import rates
from spikelib import run
import analyze

BASE = {"SPIKE_ROWS": 10000, "SPIKE_WEIGHT": 1}
VARIANTS = [
    ("default", {}),
    ("basic_loop", {"QSG_RENDER_LOOP": "basic"}),
    ("d3d12", {"QSG_RHI_BACKEND": "d3d12"}),
    ("latency1", {"QT_D3D_MAX_FRAME_LATENCY": "1"}),
    ("latency3", {"QT_D3D_MAX_FRAME_LATENCY": "3"}),
    ("no_vblank_thread", {"QT_D3D_NO_VBLANK_THREAD": "1"}),
    ("simple_driver", {"QSG_USE_SIMPLE_ANIMATION_DRIVER": "1"}),
    ("flip_discard", {"QT_D3D_FLIP_DISCARD": "1"}),
]

def main(tag):
    for name, env in VARIANTS:
        for kind, speed in (("wheel", 10), ("touchpad", 4000), ("fling", 8000)):
            r = measure(f"{tag}_diag_{name}_{kind}", {**BASE, **env}, scroll_script(kind, speed),
                        extra={"diag": name, "input": f"{kind}:{speed}"})
            print(name, kind, short(r), "untilDisplayed p50", r.get("until_displayed_p50_ms"), flush=True)
        d = run(f"{tag}_diag_{name}_animators", {**BASE, **env, "SPIKE_ANIMATORS": 1}, None, mode="pm", tail=4)
        print(name, "animators", rates(d), flush=True)
        with open(SUMMARY.parent / "diag.jsonl", "a") as f:
            print(json.dumps({"tag": tag, "variant": name, "animators": rates(d)}), file=f)


if __name__ == "__main__":
    main(sys.argv[1])
