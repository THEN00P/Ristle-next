"""Scenario 2 scrollbar check at 1M rows: drag the thumb to the bottom, then to
the middle, then wheel once. Snapshots of the table after each step are read
back by eye (row numbers) and the duplication pass records whether the list
followed the thumb in every frame. Usage: scrollbar.py <tag> [rows]"""
import sys
import analyze
from scenarios import ORIGIN
from spikelib import Script, TITLE, display, run

if __name__ == "__main__":
    tag = sys.argv[1]
    rows = int(sys.argv[2]) if len(sys.argv) > 2 else 1_000_000
    k = int(display()["scale"]) / 100
    X = lambda v: round((ORIGIN[0] + v) * k)
    Y = lambda v: round((ORIGIN[1] + v) * k)
    thumb_x, top, bottom = 925, 170, 696   # logical client coordinates
    s = Script()
    s.add(0, "focus", TITLE)
    s.mouse_drag(1000, X(thumb_x), Y(top), X(thumb_x), Y(bottom), 1000)
    s.mouse_drag(3500, X(thumb_x), Y(bottom - 8), X(thumb_x), Y((top + bottom) // 2), 600)
    s.add(5500, "move", X(600), Y(450))
    s.add(5600, "wheel", -120)
    d = run(f"{tag}_scrollbar", {"SPIKE_ROWS": rows, "SPIKE_WEIGHT": 1}, s, mode="dd",
            rects={"list": (248, 150, 690, 560)}, snaps=[900, 3200, 5300, 6900])
    print(d, analyze.content_cadence(d, "list"))
