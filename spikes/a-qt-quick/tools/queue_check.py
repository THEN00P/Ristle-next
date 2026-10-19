"""Per-frame corruption check for the animated queue (scenario 1).

The app logs every queue edit into its ring buffer; replaying them gives the
model order at any QPC time. Every duplicated desktop frame carries the
decoded marker runs (y range and item id) of the queue's left-edge strip.

The run's recorder must not have missed a desktop frame (`recorder_missed`
is 0); otherwise a clean result is not certified. The ghost deadline is the
transition plus four refreshes, not two: an edit is logged on the GUI thread
and reaches the screen 2 to 3 refreshes later (measured), so two would flag
every normal removal.

A frame is *settled* when no edit happened within the transition time plus
four refresh periods (presentation latency), and no row is held by a drag. Checks:

  settled frames  rows form one contiguous run sequence with the row pitch,
                  their ids are a contiguous slice of the model order, every
                  marker is fully opaque (a decoded run of full row height),
                  and nothing else is visible (no ghosts)
  every frame     no id is visible twice, and no id is visible that left the
                  model longer ago than the transition time plus two frames
"""
from __future__ import annotations

import bisect
import analyze

EV = analyze.EV


def replay(rg):
    """Yields (qpc, order) after every edit, starting with the reset."""
    order, states = [], []
    for q, k, a, t in rg["events"]:
        if k == EV["Q_RESET"]:
            order = list(range(1, a + 1))
        elif k == EV["Q_INS"]:
            order.insert(a >> 16, a & 0xFFFF)
        elif k == EV["Q_REM"]:
            idx, id_ = a >> 16, a & 0xFFFF
            assert order[idx] == id_, (idx, id_, order[idx])
            order.pop(idx)
        elif k == EV["Q_MOVE"]:
            f, to = a >> 16, a & 0xFFFF
            order.insert(to, order.pop(f))
        else:
            continue
        states.append((q, list(order), k))
    return states


def parse_runs(s):
    runs = []
    for part in s.split():
        span, id_ = part.split(":")
        y0, y1 = span.split("-")
        runs.append((int(y0), int(y1), int(id_)))
    return runs


def check(d, transition_ms, row_px, drag_windows=()):
    m = analyze.meta(d)
    hz = analyze.refresh_hz(m)
    T = 1000 / hz
    rg = analyze.ring(d)
    states = replay(rg)
    times = [q for q, _, _ in states]
    removed_at = {}
    prev = None
    for q, order, k in states:
        if prev is not None:
            for id_ in set(prev) - set(order):
                removed_at[id_] = q
        prev = order
    _, cols, rows = analyze.dupcap(d)
    # Transition time plus presentation latency (2-3 frames measured) plus one.
    settle = (transition_ms + 4 * T) * 1e4
    problems, settled_frames, frames = [], 0, 0
    dim_since = {}  # fake id -> first qpc it was seen in an unbroken streak
    for r in rows:
        q = r["present_qpc"]
        i = bisect.bisect_right(times, q) - 1
        if i < 0:
            continue
        frames += 1
        order = states[i][1]
        runs = parse_runs(r["markers"])
        ids = [x[2] for x in runs]
        # Every frame: duplicates and ghosts.
        full = [x for x in runs if x[1] - x[0] + 1 >= row_px - 2]
        dup = {x for x in [y[2] for y in full] if [y[2] for y in full].count(x) > 1}
        if dup:
            problems.append((q, "duplicate", sorted(dup)))
        fakes = set()
        for id_ in set(ids) - set(order):
            gone = removed_at.get(id_)
            if gone is None:
                # Not an id the model ever had: a row blended at partial
                # opacity whose colour passes the checksum. Normal mid-fade;
                # a defect only if it outlives the transition.
                fakes.add(id_)
                first = dim_since.setdefault(id_, q)
                if q - first > settle:
                    problems.append((q, "dimmed", id_, round((q - first) / 1e4, 1)))
            elif q - gone > settle:
                problems.append((q, "ghost", id_, round((q - gone) / 1e4, 1)))
        for id_ in list(dim_since):
            if id_ not in fakes:
                del dim_since[id_]
        in_drag = any(a <= q <= b for a, b in drag_windows)
        if q - states[i][0] < settle or in_drag:
            continue
        settled_frames += 1
        # Settled: contiguous, full height, ordered slice of the model.
        inner = runs[1:-1] if len(runs) > 2 else []
        partial = [x for x in inner if abs((x[1] - x[0] + 1) - row_px) > 2]
        if partial:
            problems.append((q, "partial-row", partial[:3]))
        if full:
            start = order.index(full[0][2]) if full[0][2] in order else None
            if start is None or [x[2] for x in full] != order[start:start + len(full)]:
                problems.append((q, "order", [x[2] for x in full][:12], order[:12]))
            gaps = [(a[1], b[0]) for a, b in zip(full, full[1:]) if b[0] - a[1] > 2]
            if gaps:
                problems.append((q, "gap", gaps[:3]))
    kinds = {}
    for p in problems:
        kinds[p[1]] = kinds.get(p[1], 0) + 1
    # Problems in the final settled second (after every edit stopped): damage
    # that never heals.
    end = rows[-1]["present_qpc"] if rows else 0
    final = [p for p in problems if p[0] > end - 1e7 and p[0] - states[-1][0] > settle]
    missed = sum(1 for r in rows if r["accumulated"] > 1)
    # `problems` counts findings (a frame out of order usually also has a gap);
    # the frame counts below count each captured frame once.
    return {"run": d.name, "frames": frames, "settled_frames": settled_frames, "edits": len(states) - 1,
            "problems": len(problems), "by_kind": kinds, "persistent_at_end": len(final),
            "frames_with_problems": len({p[0] for p in problems}),
            "frames_broken_at_end": len({p[0] for p in final}),
            "problem_qpcs": sorted({p[0] for p in problems}),
            "recorder_missed": missed,
            # Only a capture without a single missed desktop frame can certify a
            # run as clean (method rule 2).
            "certified_clean": not problems and missed == 0,
            "examples": [str(p) for p in problems[:8]]}
