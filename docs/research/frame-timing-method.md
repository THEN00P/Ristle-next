# Measuring UI smoothness so that a lag cannot hide

This is the method spike A must use to certify Qt Quick, and later any UI. An
agent cannot see a dropped frame, and a timer or recorder can be blocked by the
very stall it is meant to measure. A result counts only if it is collected under
all of the rules below.

## Rule 1: the clock lives outside the measured thread

- **Primary evidence is the OS's record of presented frames.**
  On Windows, use **PresentMon**. It reads ETW events from the graphics stack and
  logs every `Present` call with its QPC timestamps, the time each frame actually
  reached the display, and frames that were dropped. It runs in another process,
  so an app stall cannot delay its clock.
  On macOS, use Instruments (Animation Hitches and Core Animation) once a Mac is
  available.
- **Secondary evidence is in-app timestamps, and it is used only for
  attribution.** On the render thread, at `QQuickWindow::afterRendering` and
  `frameSwapped`, and on the GUI thread at `beforeSynchronizing`, read the
  monotonic `QElapsedTimer` / QPC and write it into a **preallocated ring
  buffer**. No logging, allocation or I/O happens inside the loop. The buffer is
  dumped after the run and correlated with PresentMon through QPC. If the two
  disagree, PresentMon wins.
- `QSG_RENDER_TIMING=1` and the QML profiler are useful for diagnosis, never as
  proof.

## Rule 2: record every frame, never sample

- PresentMon logs every present. Check that the logged frame count matches the
  expected count (refresh rate × duration). A gap in the log is itself a
  failure.
- Screenshots and video are **not** timing evidence. For visual corruption, such
  as the ListView transition corruption in QTBUG-49868, capture **every desktop
  frame** through the DXGI Desktop Duplication API. Each captured frame carries a
  DWM timestamp, and the count is checked against the refresh rate. A recorder
  that can skip frames can skip the corrupted one.
- **Input-to-photon latency:** inject input with `SendInput` and record the QPC
  time of the injection. Find the first duplicated desktop frame where the screen
  changed in the expected region. Both timestamps are on the QPC clock and
  outside the app.

## Rule 3: prove the detector with known lag first (positive control)

Before any measurement counts, the spike app has a debug hook that, on command,
**blocks the GUI thread** for exactly 4, 8, 16, 33 and 100 ms, and separately
**blocks the render thread** for the same durations. These are fired during a
scroll and during an animation.

The pipeline must detect **every** injected stall, attribute it to the right
thread, and report its duration within one refresh interval. If it misses an
injected 8 ms stall, it cannot certify the absence of real ones, and every
later "no drops" result is void.

## Rule 4: find the breaking point; "no lag" alone proves nothing

Everything breaks somewhere. Each scenario is run as a **load ramp** until frame
drops appear. Report the **knee**, the load where drops start. A run that never
produced a drop did not prove anything and has to be repeated with more load.

Load axes, one at a time and then combined:

| Axis | Ramp |
|---|---|
| Rows in a list | 1k → 10k → 100k → 1M |
| Delegate weight | text only → artwork → artwork + glow + hover states |
| Model churn | 1 → 10 → 100 → 1000 inserts, removes or moves per second during scroll |
| Fling velocity | scripted at fixed px/s, up to the maximum the OS produces |
| Artwork | cached → uncached decode, concurrency 1 → 16 |
| CPU budget | all cores → process affinity to 2 cores → 1 core, plus the OS power-saver plan |
| GPU | discrete → integrated → software rendering (WARP / `QSG_RHI_BACKEND`) |

**Pass:** the production envelope must sit at least **4× below the knee** on
the user's machine, and must have no drops at all under software rendering, with
1,000 rows and artwork.

## Rule 5: script the input, fix the conditions

- Timing runs are driven by scripted `SendInput` sequences (flings, drags,
  wheel and touchpad streams) at fixed velocities and timings. They are
  repeatable, and they are never driven by a human or by an AI agent.
- Every run is repeated at 60 Hz and at the display's highest rate, and at 100 %,
  125 % and 150 % scaling. A cold app start and a warm app start are separate
  runs.
- Background noise is recorded: other processes' CPU use and the power plan.

## Rule 6: compare against baselines with the same harness

- **Ristle legacy**, in the same scenarios. It is expected to drop frames, so it
  also shows that the harness catches real-world lag and not only injected
  stalls.
- **A native reference**, such as File Explorer or Settings scrolling a long
  list, defines what smooth means on that machine.

## Rule 7: long runs

A 30-minute scripted soak combines scrolling, queue edits and page switches.
Frame statistics and memory are tracked over time. Pass: no upward trend in
drops, and memory flat after warm-up.

## Report format

For every scenario report the knee per axis, p50/p99/max frame time, dropped
frames per minute at the production envelope, the input-to-photon p50 and p99,
the injected-stall detection table, and the baseline numbers. Raw PresentMon CSVs
and ring-buffer dumps are committed with the report.
