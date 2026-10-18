# Qt Quick for the desktop app (spike A)

Spike A, 2026-10-17 to 2026-10-18. Code and harness: `spikes/a-qt-quick/`
(deleted after review, except what phase 4 keeps, see the end). Question from
ADR 0002: is Qt Quick smooth enough for Ristle's desktop app, measured with
`docs/research/frame-timing-method.md`, or does GPUI have to be evaluated?

**Verdict: no-go as the method defines it, so evaluate GPUI.** Two binding
criteria fail and a third is not met within an hour:

1. **Software rendering.** The method requires no dropped frames with 1,000
   rows and artwork under software rendering. Every software path Qt offers
   drops frames while scrolling. WARP drops 140 to 1,700 frames per minute.
   Mesa llvmpipe freezes the GUI clock for 5 to 31 frames per 9-second run at
   60 Hz and skips more than 1,000 refreshes per minute of motion at 144 Hz.
   The Qt Quick software adaptation skipped 3 to 30 GUI-clock frames per run
   at 60 Hz (exploratory runs only). The cause is structural. Qt Quick redraws the whole window every frame,
   and a CPU rasteriser cannot do that for a 1280 × 800 window at 60 Hz.
2. **Fresh page creation at 144 Hz sits at the knee.** Building a page that
   was not cached costs 3 to 5 frames at 144 Hz (22 dropped frames per minute
   with artwork rows, 71 with per-row glow), so the production envelope is not
   4× below its knee. Cached pages switch without drops.
3. **Memory is not flat after warm-up.** In a 60-minute soak on D3D12 private
   memory rose 13 MB in the first 20 minutes and 3 MB in the last 35. It is
   levelling off, but not flat, and the cause is not known.

Everything else passes on the discrete GPU, but only with four settings that
are not Qt's defaults:

| Setting | Why | Without it |
|---|---|---|
| D3D12 backend (`QSG_RHI_BACKEND=d3d12`) | Qt's default D3D11 backend drops single frames at 144 Hz | 20 to 50 dropped frames per minute at any load, 734 in the 30-minute soak (13 with D3D12) |
| No `Animator` types | A running render-thread Animator halves the GUI thread's frame rate on every flip-model backend | Scrolling, list transitions and the sidebar run at 27 of 60 or 67 of 144 frames per second while PresentMon reports a perfect present rate |
| Queue transitions drive opacity and scale home in `displaced` (Qt's documented "interrupted transitions" pattern) | Plain add/remove/displaced transitions corrupt the list when edits interrupt each other (QTBUG-49868 reproduced) | Rows left dimmed, overlapping or missing, permanently, from 5 edits per second |
| `NativeRendering` for text | The default distance-field text is softer than every native renderer | Acutance 0.42 instead of 0.53 at 100 % |

None of these is a workaround of the kind Elisa carries: transitions stay on,
and all four are one line of configuration or a documented QML pattern. ADR
0002's own trigger ("animated list insert, remove and reorder, or Windows text
sharpness, can only be achieved with workarounds of the kind Elisa carries")
is therefore not met. The no-go comes from the method's stricter software
rendering, 4× and memory criteria, which ADR 0002 did not anticipate. I recommend
running GPUI through the same harness before phase 4. If GPUI fails the
software criterion as well, which is likely for any GPU-first toolkit, the
criterion needs revisiting with the user, and Qt Quick with the four settings
is then the best measured option. See "Recommendation" at the end.

## Setup

- Windows 11 Home 26200, Ryzen 7 7700X (8 cores, 16 threads), 16 GB, NVIDIA
  RTX 4070 (driver 32.0.16.1088) driving a 2560 × 1440 panel at 59.951 Hz and
  143.856 Hz, AMD Radeon iGPU (Raphael, no display attached, so iGPU runs copy
  across adapters), High performance power plan unless a run says otherwise.
  Scaling 100 %, 125 % and 150 %, restored to 100 % and 59.951 Hz afterwards.
- Qt 6.11.3 (current LTS) from aqtinstall, msvc2022_64. cxx-qt 0.10.0, Rust
  1.99.1, MSVC 14.51. Release builds.
- The app (`spikes/a-qt-quick/app/`) is a Lumen desktop shell: sidebar, a
  track table (header, `#`, title and artist with artwork, album, date added,
  duration, hover actions), a library grid of MediaCards, a queue panel with
  animated insert, remove and drag reorder, and a now-playing bar with the
  amber MultiEffect glow on the play control. Rust owns all models through
  cxx-qt (`TrackModel` with up to 40M rows as seeds, `QueueModel`, a `Spike`
  config singleton). QML handlers call one core method each. Artwork decodes
  on a thread pool behind a `QQuickAsyncImageProvider`, which also rounds the
  corners so each row is one texture. Text renders with `NativeRendering`,
  Segoe UI for body text and the bundled Schibsted Grotesk and DM Mono.
- Load knobs are environment variables (`SPIKE_ROWS`, `SPIKE_WEIGHT`,
  `SPIKE_CHURN`, `SPIKE_QUEUE_OPS`, `SPIKE_ART_CACHE`, `SPIKE_ART_THREADS`,
  `SPIKE_GLOW`, `SPIKE_FRESH_PAGES`, `SPIKE_ANIMATORS`, `SPIKE_QUEUE_FIX`).
  Delegate weight 0 is text only, 1 adds artwork, 2 adds a MultiEffect glow
  per row plus hover states, 3 uses a `RectangularShadow` glow instead.

Run counts: 872 measured runs (567 PresentMon passes, the rest duplication
passes) in the main matrix (D3D11, six conditions), the D3D12 pass (four
conditions), the diagnosis and the extension. Background CPU was 0.6 to 28 % (median about 3 %, recorded per run in
`results/summary.jsonl`). No emulator, Gradle or build ran during timing.
Raycast, T3 Code, ChatGPT, Claude and Wispr Flow stayed open.

macOS: the `spike-a` job in `spike-c-ios.yml` (run 39109678018, macos-15)
builds the same app against Qt 6.11.3 and runs it for 6 s. It used the Metal
backend and rendered 362 frames with no QML errors. No timing on macOS; that
waits for the user's Mac.

## The harness

All of it is out of process except the ring buffer, and every timestamp is QPC
(10 MHz on this machine) so the sources line up.

- **PresentMon 2.6.0** (release exe, no installer, no elevation: the account
  is in Performance Log Users) records every present of the app by process
  id. This is the timing evidence.
- **`harness dupcap`** acquires every desktop frame through DXGI Desktop
  Duplication and logs DWM's present time, the accumulated-frame count, a hash
  per region, decoded marker strips and two clock pixels. It is used for
  corruption, input-to-photon and content cadence, and for apps that present
  through composition (File Explorer). An accumulated count above 1 means the
  recorder itself missed a frame. That happened for 476 of 336,697 frames
  (0.14 %), about 3 per run on the hardware path. A missed frame looks like a
  skipped refresh in the cadence numbers, so Qt's cadence figures below are
  upper bounds.
- **`harness input`** sends `SendInput` keys, mouse and wheel events and
  `InjectTouchInput` touch drags at fixed times (sleep, then spin for the last
  2 ms) and logs the QPC time of each.
- **The app's ring buffer** (`app/cpp/probe.cpp`) is preallocated and
  pre-touched, and records QPC at `afterAnimating` (GUI thread),
  `beforeSynchronizing`, `afterSynchronizing`, `beforeRendering`,
  `afterRendering` and `frameSwapped` (render thread), input arrival, injected
  stalls and every queue edit. It is dumped after the run and only used to
  attribute a frame to a thread and to replay the queue order.
- `harness display` sets refresh rate and scaling (the DPI-scale device-info
  packets the Settings app uses) and reports the exact refresh as a rational.
  `harness procstat` samples memory and CPU. `harness textref` draws the native
  text reference.
- Python drivers in `tools/` run each configuration as a fresh app launch,
  check for heavy processes, record background CPU, power plan and display
  mode, and write a summary line per run.

Four things about the tools themselves shaped the method and are worth knowing
before the next spike:

1. **PresentMon cannot see GUI-thread stalls while a render-thread Animator
   runs.** Qt's threaded render loop keeps rendering animator-only frames when
   the GUI thread is blocked. A 100 ms GUI stall left no trace in PresentMon
   with the queue's equaliser glyph (a `ScaleAnimator`) running. Measured with
   Animators off, every present implies a GUI sync. This is also the reason for
   the "no Animators" setting above.
2. **PresentMon loses a process's presents while any Desktop Duplication
   session is active** (reproduced three times: a 2,034 ms gap logged as one
   frame while the app swapped 60 frames per second according to its ring
   buffer). Every scenario therefore runs twice with the same input script: a
   PresentMon pass and a duplication pass.
3. **A touch-injecting process must outlive the measurement.** When it exits,
   Windows removes the virtual touch device and Qt rebuilds its input-device
   list on the GUI thread, which costs about 15 ms and dropped a frame at the
   end of every early run. The injector now waits until the recorders stop,
   and a warm-up touch before the measured window absorbs Qt's first-touch
   cost.
4. **Desktop Duplication shows what reached the screen, not how long a thread
   stalled.** At 144 Hz the presentation queue absorbs a 4 ms stall entirely
   and shows a 16 ms stall as one skipped refresh. That is real (the user sees
   less than the stall), but it makes duplication unfit as the stall detector.

## Positive control

Stalls were injected with keys: F5 to F9 block the GUI thread for 4, 8, 16, 33
and 100 ms inside the next frame (at `afterAnimating`, where binding updates
and delegate creation cost time), Shift+F5 to F9 block the render thread at
the next `beforeRendering`. Each set ran during the idle heartbeat animation
and during continuous touch drags over the table. The measured stall is the
largest excess of `MsBetweenPresents` over the run's median within the stall
window. The ring buffer attributes it to the thread whose phase grew.

**PresentMon detected and attributed 20 of 20 stalls in every one of the ten
conditions** (six D3D11, four D3D12). The worst error was 1.3 ms (a 33 ms
render stall at 144 Hz / 150 % measured as 31.7 ms), against a tolerance of one
refresh (6.95 or 16.7 ms). Full table: `results/tables.md`, section "Positive
control".

| Condition | During | GUI thread, injected → measured (ms) | Render thread |
|---|---|---|---|
| 144 Hz / 100 %, D3D12 | animation | 4→4.3, 8→8.0, 16→16.2, 33→33.2, 100→100.3 | 4→4.1, 8→8.0, 16→15.9, 33→33.3, 100→100.1 |
| 144 Hz / 100 %, D3D12 | scroll | 4→3.8, 8→8.1, 16→16.5, 33→33.1, 100→100.2 | 4→4.2, 8→8.2, 16→15.7, 33→33.3, 100→100.3 |
| 60 Hz / 100 %, D3D11 | animation | 4→3.9, 8→8.0, 16→16.0, 33→33.1, 100→100.1 | 4→4.1, 8→8.5, 16→16.2, 33→33.1, 100→100.0 |
| 60 Hz / 100 %, D3D11 | scroll | 4→5.0, 8→7.1, 16→16.4, 33→32.6, 100→100.3 | 4→3.6, 8→8.4, 16→15.4, 33→33.3, 100→99.8 |

Two caveats. A GUI stall at a random moment between frames (Ctrl+F5 to F9)
often lands in the GUI thread's idle slack and never touches a frame. Those
are invisible to any frame-based measurement and also to the user. And the
control is only valid with Animators off (point 1 above).

## Findings outside the scenarios

### Animators halve the GUI thread's frame rate

With one `ScaleAnimator` running, the GUI thread ticks every other vsync:

| Backend | GUI ticks per second | Frames per second |
|---|---|---|
| D3D11 (default), 60 Hz | 27.6 | 56.2 |
| D3D12, 60 Hz | 26.8 | 53.9 |
| Vulkan, 60 Hz | 24.8 | 49.7 |
| OpenGL (copy-model swap chain), 60 Hz | 56.0 | 56.4 |
| D3D11 / D3D12, 144 Hz | 66.8 / 66.2 | 134.0 / 132.9 |
| Basic render loop, 144 Hz | 134.0 | 134.0 (but see below) |

(The first second of each run is startup, so per-second rates sit below the
refresh rate.) `QSG_RENDER_TIMING` shows the mechanism: the render thread's
animator-only frame blocks about one refresh on the swap chain inside its sync
phase, the GUI thread's sync request waits behind it (`blockedForSync=16 ms`),
and every GUI frame spans two vsyncs. Frame latency 1 or 3, flip-discard, the
simple animation driver and turning off Qt's vblank thread did not change it.
Every GUI-driven motion (scrolling, list transitions, the sidebar) runs at
half rate while any Animator anywhere in the window runs, and PresentMon
reports a full present rate. ADR 0002's rule "use Animator types for motion
that must survive a busy GUI thread" has to be reversed: no Animators.

### D3D11 drops single frames at 144 Hz

At 143.856 Hz the default backend dropped 20 to 50 frames per minute at every
load, including 1,000 rows and slow flings. Each drop was one frame in which
the GUI phase (from `afterAnimating` to sync) lasted exactly one refresh
interval: a wait on the render thread's throttle, not work. The diagnosis at
base load (wheel, touchpad, fling):

| Variant | Wheel | Touchpad | Fling |
|---|---|---|---|
| D3D11 (default) | 40/min | 33/min | 0 |
| **D3D12** | **0** | **0** | 7/min |
| `QT_D3D_MAX_FRAME_LATENCY=1` | 40 | 106 | 15 |
| `QT_D3D_MAX_FRAME_LATENCY=3` | 13 | 20 | 0 |
| `QT_D3D_NO_VBLANK_THREAD=1` | 27 | 40 | 7 |
| `QSG_USE_SIMPLE_ANIMATION_DRIVER=1` | 27 | 13 | 0 |
| `QT_D3D_FLIP_DISCARD=1` | 20 | 33 | 7 |
| `QSG_RENDER_LOOP=basic` | 4,217 | 4,705 | 4,553 (renders at 72 frames per second) |

The 30-minute soak confirms it: 734 dropped frames on D3D11, 13 on D3D12. The
cost of D3D12 is memory, 311 MB private at idle instead of 150 MB, and 60 ms
more to the first frame (590 ms instead of 530 ms). At 60 Hz the backends
behave the same.

## Scenarios

Production envelope used for the 4× rule: a 10,000-row playlist with artwork
(weight 1) and hover states, artwork decoded on 4 threads, library sync
editing the visible list at most once per second, queue edits at most 5 per
second, flings up to Qt's 12,000 px/s cap, wheel and touchpad at any rate,
all cores, the discrete GPU, 60 and 144 Hz, 100 to 150 %. Results below are
D3D12 at 144 Hz unless marked. D3D11 and 60 Hz tables are in
`results/tables.md` and `results/ramps.md`.

### 1. Animated queue

Each run: two mouse drag reorders, then 6 s of random inserts, removes and
moves by the core at the given rate, then 12 keyboard edits 50 ms apart (the
QTBUG-49868 pattern of transitions interrupting each other), then rest. Every
queue row carries an 8 px two-colour strip that encodes its id with a 32-bit
check. Every duplicated frame is decoded and compared against the model order
replayed from the ring buffer. A frame counts as settled 220 ms (the
transition) plus four refreshes after the last edit.

| Edits/s | Transitions | Dropped frames/min | Corruption frames | Still broken after edits stop |
|---|---|---|---|---|
| 1 | settling | 13 | 0 | 0 |
| 5 | settling | 0 | 0 | 0 |
| 20 | settling | 0 | 1 (one ghost frame) | 0 |
| 50 | settling | 0 | 0 | 0 |
| 100 | settling | 0 | 798 | **286 frames** |
| 200 | settling | 4 | 164 | 0 |
| 1 | plain | 0 | 0 | 0 |
| 5 | plain | 4 | 944 | **288 frames** |
| 20 | plain | 4 | 812 | **288 frames** |

Plain transitions (opacity and scale in `add`, opacity in `remove`, x and y in
`displaced`) break at 5 edits per second at every condition, 60 Hz included:
rows stay at partial opacity, overlap, or leave gaps, and the damage never
heals. Snapshots of it are in `results/raw/diagnostics.7z`
(`diag_queue20_snaps/`). With the documented
pattern (the displaced transition also animates opacity and scale to 1) the
queue is clean through 50 edits per second at all conditions. The knee is
between 50 and 100, at least 10× above the envelope. The 200-per-second run
healed, the 100-per-second run did not, so the knee itself is not sharp.
Drag reorder never produced a problem.

Input-to-photon for an insert from the keyboard (first key of each burst,
SendInput to the DWM present of the first changed frame): median 24 ms at
144 Hz, 39 to 49 ms at 60 Hz.

### 2. Track table, ramped to 1M rows and beyond

Knee per axis, D3D12 at 144 Hz (each step is an 8 s fling, wheel or touchpad
run; a drop is a present interval above 1.5 refreshes):

| Axis | Ramp | Knee | Envelope vs knee |
|---|---|---|---|
| Rows | 1k, 10k, 100k, 1M, 10M, 40M | none (0 drops at 40M) | 10k |
| Delegate weight | text, artwork, RectangularShadow glow, MultiEffect glow per row | none (0 to 2 drops per run) | artwork + hover |
| Model churn during fling | 1, 10, 100, 1,000, 3,000, 10,000, 30,000 per s | 3,000 per s (209/min) | 1 per s, 3,000× below |
| Fling velocity | 1,000 to 12,000 px/s (Qt's cap) | none | |
| Wheel, touchpad | 5 to 40 notches/s, 1,000 to 20,000 px/s | none | |
| Artwork | cached, uncached on 1, 4, 16 threads | none | |
| CPU | all cores, 2 cores, 1 logical core, power saver, both, then with MultiEffect per row and churn up to 1,000/s | none alone (1 drop per run on 1 core with power saver, MultiEffect per row and 1,000 churn/s) | all cores |
| GPU | RTX 4070, iGPU (across adapters), WARP | iGPU (112/min) | discrete only |
| Combined | 10M rows, MultiEffect per row, uncached art, 1,000 churn/s, 1 core, power saver | breaks: 6,460/min, p50 23 ms (artwork decode threads share the one core) | every factor far above the envelope |

At 60 Hz no CPU-side axis dropped a frame through the method's ramp ranges.
Only the combined ramp broke there, with 1M rows, MultiEffect per row, 100
churn ops/s and uncached artwork pinned to one logical core (1,180 drops/min).

The scrollbar works at 1M rows (`c59s100_scrollbar/` in
`results/raw/c59s100.7z`): dragging the thumb
to the bottom shows rows 999,990 to 1,000,000, dragging it to the middle lands
on row 496,918, and a wheel notch then moves two rows. The 4 % minimum thumb
size keeps it grabbable. The `#` column, 32 px wide in the spike, clips
numbers from six digits up; the real table has to size it. Frame-time percentiles do not move with
row count: p50 6.9 ms, p99 7.4 ms, max 8.1 to 8.8 ms from 1k to 1M rows.

### 3. Sidebar toggle over a large list

Twelve toggles of the animated sidebar width (200 ms) while the table relayouts
every frame:

| Load | 144 Hz D3D12, drops/min | 60 Hz, drops/min |
|---|---|---|
| 1M rows, artwork | 0 | 0 |
| 1M rows, MultiEffect per row | 0 | 0 |
| same, one logical core | 0 | 76 |
| same, WARP | 344 | 186 |

Input-to-photon from Ctrl+B to the first changed sidebar frame: 11 to 19 ms at
144 Hz, 21 to 31 ms at 60 Hz.

### 4. Page switches

Twelve switches between the track table and the library grid, 900 ms apart:

| Pages | Rows | 144 Hz D3D12, drops/min (max frame ms) | 60 Hz, drops/min |
|---|---|---|---|
| cached | artwork | 0 (8.1) | 0 |
| cached | MultiEffect per row | 0 (8.6) | 0 |
| fresh | artwork | 22 (11.5) | 0 |
| fresh | MultiEffect per row | 71 (20.5) | 5 |
| fresh, one logical core | MultiEffect per row | 104 (25.4) | 82 |

A fresh page builds every visible delegate in one frame. At 144 Hz that is
3 to 5 refreshes. This is the second criterion that fails: the envelope (first
visit to a page with artwork rows) is at the knee. Keeping pages alive
(`StackLayout` with visibility, as `Main.qml` does) removes it, and
`ListView`'s `asynchronous` delegate incubation would spread the cost. Both are
phase 4 design rules rather than proof that the first visit is smooth.
Input-to-photon: 11 to 23 ms at 144 Hz, 22 to 47 ms at 60 Hz.

### 5. Text sharpness

The Qt specimen and a native reference window (same strings, sizes, colours
and positions; DirectWrite ClearType, DirectWrite grayscale, GDI ClearType)
were cropped from duplicated frames at each scale. Acutance is the mean
horizontal luminance gradient over ink pixels divided by the text contrast;
solid is the share of ink pixels within 25 % of full contrast. White band,
Segoe UI 12 to 14 px, averaged:

| Scale | Qt distance field (default) | Qt native | Qt curve | GDI ClearType | DirectWrite ClearType | DirectWrite grayscale |
|---|---|---|---|---|---|---|
| 100 % | 0.42 / 0.33 | 0.53 / 0.52 | 0.50 / 0.42 | 0.53 / 0.47 | 0.44 / 0.35 | 0.47 / 0.39 |
| 125 % | 0.38 / 0.34 | 0.45 / 0.49 | 0.44 / 0.41 | 0.43 / 0.41 | 0.44 / 0.46 | 0.46 / 0.47 |
| 150 % | 0.38 / 0.38 | 0.44 / 0.52 | 0.41 / 0.46 | 0.43 / 0.44 | 0.42 / 0.48 | 0.43 / 0.48 |

Each cell is acutance / solid. Colour fringing (mean |R−G|+|G−B| over ink) is
about 110 for every ClearType renderer including Qt native, 85 to 90 for Qt's
distance field and 0 for the grayscale ones. Line widths of Qt native match
GDI at 100 % (148/160/183 px) and DirectWrite at 125 and 150 %.

Qt's `NativeRendering` reproduces GDI ClearType at 100 % (same line widths,
acutance 0.53 against 0.53), which is what classic Win32 lists such as File
Explorer use, and is at least as crisp as every native renderer at 125 and
150 %. The default distance-field `QtRendering` is the softest renderer at
every scale. `CurveRendering` is grayscale and sits in between. Comparison sheets (×4, nearest neighbour):
`results/text_c59s100.png`, `text_c59s125.png`, `text_c59s150.png`. ADR 0002's
text trigger is not met.

### 6. Lumen glow through MultiEffect

The play control's amber glow (`MultiEffect` shadow, blur 1.0, blurMax 32) and,
at level 2, a 64-px blurred artwork backdrop behind the transport bar, while
flinging:

| GPU | Glow off | Glow | Glow + blurred backdrop |
|---|---|---|---|
| RTX 4070, 144 Hz D3D12: GPU busy p50 / p99 ms, drops/min | 2.0 / 6.2, 0 | 2.1 / 5.6, 0 | 2.4 / 6.3, 0 |
| iGPU, 144 Hz: drops/min | 157 | 201 | 134 |
| WARP, 144 Hz: drops/min | 1,021 | 753 | 253 |

The glow costs about 0.1 ms of GPU time and no frames on the discrete GPU.
GPU busy at 60 Hz reads 5.7 ms with or without glow because the GPU sits at a
low clock. The iGPU fails at 144 Hz with or without glow. Its frames cross
adapters here, so this says little about a laptop iGPU driving its own panel.

### 7. Idle memory and the soak

Idle with the 1,000-row page open (ring buffer reduced to 4,096 entries),
60 s after launch: 150 MB private, 150 MB working set, 0 to 0.4 % CPU on
D3D11; 311 MB private on D3D12. Without the heartbeat animation, CPU is 0.

The soak cycles every minute through 20 s of flings, 10 s of queue edits by
the core at 5 per second, eight page switches, six sidebar toggles and wheel
scrolling, at 144 Hz with the 10,000-row table. Memory includes the 96 MB
ring buffer, pre-touched at launch.

| Run | Dropped frames | Per minute | Private memory after warm-up |
|---|---|---|---|
| D3D11, 30 min | 734 (352 first half, 382 second) | 12 to 32, flat | 261 → 264 MB, +0.18 MB/min |
| D3D12, 30 min | 13 (7, 6) | 0 to 3 | 426 → 435 MB, +0.48 MB/min |
| D3D12, 60 min | 25 (16, 9) | 0 to 3 | 427 → 437 MB, +0.16 MB/min |

Drops show no upward trend. Memory is not flat. In the 60-minute run it rose
13 MB in the first 20 minutes and 3 MB in the last 35 (about 0.09 MB/min; the
five-minute medians from minute 25 are 432.6, 434.0, 434.1, 435.0, 435.6, 436.8
and 437.2 MB). It is levelling off, but the method's "flat after warm-up" is
not met within an hour. Whether this is caches filling (pipeline, glyph and
pixmap caches all grow with new content) or a leak needs a longer run and a
heap profile, which I did not do.

## Baselines

Ristle legacy did not start: the installed AppX is a debug build that loads
its JavaScript from Metro on `localhost:8081`, and the legacy checkout has no
installed `react-native` package to run Metro. Getting it running needs a full
`npm install` in `../Ristle`, so it was skipped as the brief allows.

File Explorer, a 3,000-file folder in Details view, placed at the same client
rect and driven by the same scripts. It presents through composition, so the
comparison metric is content cadence in the list region from the duplication
pass (refreshes skipped while the list moves, counted in the fast phase of
each fling only), plus PresentMon on `dwm.exe` by process id:

| | Fling 2,000 px/s | Fling 8,000 px/s | Updates/s while moving |
|---|---|---|---|
| Qt, 144 Hz D3D12 | 62 skipped per min of motion | 36 | 143 |
| File Explorer, 144 Hz | 4,316 | 5,499 | 54 to 67 |
| Qt, 60 Hz | 0 | 0 to 92 | 60 |
| File Explorer, 60 Hz | 352 to 566 | 1,156 to 1,483 | 44 to 55 |

Explorer's list updates at 44 to 67 frames per second during a touch fling at
any refresh rate. On this machine it is not a smoothness bar Qt has to reach.
It is far below what Qt already does.

## Recommendation

1. Treat Qt Quick as failed against the method as written, and run GPUI
   through the same harness. The harness is generic (PresentMon, duplication,
   scripted input) and only needs the ring buffer hooks re-done for GPUI.
2. Decide with the user whether the software-rendering criterion stays. It
   fails for a structural reason (full redraw every frame) that applies to any
   toolkit that renders through one GPU swap chain, so GPUI may fail it the
   same way. A toolkit that passes it would need damage-region rendering.
3. If Qt Quick goes ahead, phase 4 adopts the four settings above as rules,
   plus: pages stay alive after first visit (or build with asynchronous
   incubation), no per-row `MultiEffect` (use `RectangularShadow`, which
   costs nothing measurable), and the D3D12 memory cost is accepted.

## Raw data

`spikes/a-qt-quick/results/`: `summary.jsonl` (one line per run), the scenario
files (`control`, `queue_checks`, `latency`, `baselines`, `text`, `memory`,
`startup`, `diag`), `tables.md` and `ramps.md` (generated by
`tools/report_data.py`), the text sheets, and `raw/*.7z`, one archive per
condition with every run's PresentMon CSV, ring-buffer dump, duplication log,
input log, process samples and metadata (`d12c144s100long.7z` is the
60-minute soak, `diagnostics.7z` the Animator, software-rendering and queue
snapshot runs plus the driver logs). Summaries of the exploratory runs from
before the final build are in `results/exploratory/`; their raw files were not
kept and nothing above relies on them except the Qt Quick software
adaptation figures, which are marked.
