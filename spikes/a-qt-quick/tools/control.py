"""Positive control (method rule 3): injected GUI- and render-thread stalls of
4/8/16/33/100 ms, during an animation and during a scroll."""
import sys
from spikelib import Script, run

STALL_KEYS = ["F5", "F6", "F7", "F8", "F9"]  # 4, 8, 16, 33, 100 ms


def script(scale, during_scroll, spacing=700, repeats=1):
    s = Script()
    s.add(0, "focus", "Ristle spike A")
    t = 600
    events = []
    for rep in range(repeats):
        for shift in (False, True):
            for k in STALL_KEYS:
                events.append((t, k, shift))
                t += spacing
    if during_scroll:
        # Repeated finger drags over the track table (each 600 ms, 500 px), so
        # the list is moving under every stall.
        x = int(600 * scale)
        tt = 300
        while tt < t + 400:
            s.touch_drag(tt, x, int(700 * scale), int(250 * scale), 600)
            tt += 700
        # Stalls land at 600 + k*700 ms: the middle of a drag.
    for t, k, shift in events:
        s.key(t, k, shift=shift)
    return s


if __name__ == "__main__":
    name, scale, mode, pass_ = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
    cfg = {"SPIKE_ROWS": 1000, "SPIKE_WEIGHT": 1}
    d = run(name, cfg, script(scale, mode == "scroll"), mode=pass_,
            rects={"list": (248, 200, 680, 480), "progress": (440, 770, 400, 14)})
    print(d)
