# The 144 Hz hiccups

Research, 2026-10-19. Re-analysis of spike A's recorded PresentMon files plus
sources online; no new runs. Script: `spikes/a-qt-quick/tools/hiccups.py`
(results in `spikes/a-qt-quick/results/hiccups.json`). Background:
`docs/research/qt-quick-spike.md`, section "Display drops: a floor at 144 Hz".

**The problem.** At 143.856 Hz, Qt Quick drops 1 to 9 single frames per 8 s
on screen at every load, 12 to 43 per minute depending on backend and
scaling. At 60 Hz the same runs drop nothing. The user decided this is to be
investigated and ideally removed, but is not a blocker (ADR 0002).

**Result.** The hypothesis that DWM discards one of two presents landing in
the same refresh (phase jitter) explains 4 % of the drops. The other 93 % are
frames that were presented on time and shown, but one refresh late: their
GPU work finished after DWM's composition deadline for the next refresh. On
D3D11 that is mostly the GPU waiting for a free back buffer (Qt's D3D11 swap
chain has two). On D3D12 (three buffers) it is frames whose own GPU work
takes 4 to 7 ms instead of the usual 2 ms, against a budget of about 4 ms
from present to DWM's deadline at 144 Hz. At 60 Hz the budget is about 10 ms,
which is why 60 Hz is clean.

## The hypothesis

In composed mode DWM shows at most one new frame of a window per refresh. If
two presents land inside one refresh interval, DWM shows the newer and
discards the older, the screen keeps the previous frame for one more refresh,
and the app looks on time from its own side. The prediction: drops pair with
a discarded frame, and presents near drops sit close to the vblank edge.

## Evidence from the recorded runs

The sample is every light-load ramp run (rows, weight, artwork, churn, CPU,
fling, wheel, touchpad) on the discrete GPU: 124 runs, 16.9 minutes at
144 Hz, and 73 runs, 10.0 minutes at 60 Hz, from both backends and all
scales.

### 1. Present mode

Every one of the 648 Qt runs presented as `Composed: Flip`, with
`SyncInterval` 1, `AllowsTearing` 0 and no present flags, and the mode never
changed within a run. Qt never got hardware composition or independent flip,
so DWM composed every frame. (The one other mode in the data, `Composed: Copy
with GPU GDI`, is File Explorer's baseline.)

### 2. What each drop was

The vblank grid was recovered from the screen times of the displayed frames
(present time plus `MsUntilDisplayed`); it fits within 0.32 ms at p99. Each
drop was classified by what happened since the previously displayed frame:

| Kind | 144 Hz drops | Share |
|---|---|---|
| **Held:** every present on time and displayed, but one frame reached the screen a refresh late | 398 | 93 % |
| **Collision:** a frame presented and never displayed, no late present (the hypothesis) | 17 | 4 % |
| **Late:** a present gap above 1.5 refreshes | 14 | 3 % |

At 60 Hz there were no drops of any kind.

**Present phase.** Qt presents right after the vblank: 93 % of presents at
144 Hz and 98 % at 60 Hz fall in the first tenth of the refresh interval
(the render thread is released by its frame-latency wait at vblank, records,
and presents). The 17 collision presents also sit in the first tenth. There
is no spread of presents across the interval for phase jitter to act on: the
app's cadence matches the display's (6.9556 ms mean present interval against
a 6.9514 ms refresh, 0.29 ms standard deviation), and drops are clustered
(median 0.18 s apart), not spaced like a beat between two clocks.

**What held frames have in common.** `MsRenderPresentLatency` (from the
present call until the GPU finishes the frame) has a median of 2.0 ms for all
frames and 6.9 ms for held frames. Frames that finish more than about 0.6
refresh (about 4.2 ms) after their present miss the next composition. DWM
then shows the previous frame again, the queue shifts by one, and the frame
latency steps from 2 to 3 or 3 to 4 refreshes (392 of the 415 drops after an
on-time present) before draining back. Why the GPU finishes late differs by backend:

| | Held frames start on the GPU, after present (median) | GPU busy on held frames, median / p90 | GPU busy on all frames, median / p90 | Held drops per minute |
|---|---|---|---|---|
| D3D11 (2 swap chain buffers) | 0.86 refresh | 0.8 / 7.1 ms | 0.7 / 2.9 ms | 26 |
| D3D12 (3 buffers) | immediately | 4.3 / 7.0 ms | 2.2 / 3.8 ms | 20 |

On D3D11 the GPU sits idle for most of a refresh before it starts a held
frame, and then does little work. That is the stall Microsoft documents for
flip model: "The first time you ask the GPU to write to the swapchain back
buffer is the time that the GPU will stall waiting for the buffer to become
available" [1]. With two buffers, one is on screen and the other may still be
queued for composition. On D3D12, with a third buffer, that subtype is gone
(0.1 per minute against 12.9 on D3D11), and what remains are frames that
simply take twice the usual GPU time. Text-only rows (weight 0) drop at the
same rate as rows with artwork, so artwork uploads are not the cause.

### 3. Hybrid GPU, VRR and the 143.856 Hz rate

- **Hybrid setup.** The RTX 4070 drives the panel; the Raphael iGPU has no
  display. Qt renders on the RTX (adapter 0) in every run counted here, and
  its vblank thread (`QDxgiVSyncService`) waits on the output of the adapter
  that owns the window's monitor [5]. No cross-adapter copy is involved. The
  iGPU only appears in the runs that forced it (`QT_D3D_ADAPTER_INDEX=1`),
  which are excluded and fail for their own reason (frames copied across
  adapters).
- **G-SYNC / VRR.** Not recorded by the harness. The data shows it was not in
  effect: every display interval is a whole multiple of 6.95 ms within
  0.32 ms (p99). Under variable refresh the intervals would vary.
- **143.856 Hz.** The app's present cadence locks to it (above), so the
  fractional rate does not create a beat. One outside report is worth
  knowing: Firefox's DirectComposition path glitched only at 143.98 and
  239.76 Hz, not at 119.98 Hz and below, on an NVIDIA card in a dual-monitor
  setup; it was closed after Windows updates in May and June 2024 fixed
  DirectComposition presentation issues [8]. Nothing in our data ties the
  drops to the exact rate, but a 120 Hz run would separate "high refresh"
  from "this mode".

## What Qt does today (Qt 6.11 source)

- **D3D11**: flip-discard swap chain with `BUFFER_COUNT = 2` (hard-coded,
  "a double-buffered flip model swapchain") [2][3].
- **D3D12**: `BUFFER_COUNT = 3`, two frames in flight [4].
- **Frame latency**: both use `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT`
  by default, with `maxFrameLatency = 2` ("1-3, use 2 to keep CPU-GPU
  parallelism while reducing lag compared to tripple buffering"), waited on in
  `beginFrame()`. `QT_D3D_MAX_FRAME_LATENCY` sets it; 0 turns the waitable
  object off. It is ignored with `NoVSync` and on WARP [2][3][4].
- **Pacing**: `QDxgiVSyncService` runs one time-critical thread per monitor
  calling `IDXGIOutput::WaitForVBlank()` and drives the windows on that
  monitor from it; `QT_D3D_NO_VBLANK_THREAD=1` disables it [5].
- **Other knobs**: `QT_D3D_NO_FLIP` (legacy blt model), `QT_D3D_FLIP_DISCARD`
  (ignored now, flip-discard is the default) [2]; DirectComposition is used
  only for windows with a translucent surface format [3];
  `QT_D3D_STABLE_POWER_STATE` exists on D3D12 but only with timestamps
  enabled, and it is a global, developer-mode setting [4].

Spike A already tried the knobs at base load on D3D11 (report, "D3D11 and
D3D12 at 144 Hz", on-screen drops per minute for wheel / touchpad / fling):
default 80 / 53 / 7, D3D12 27 / 66 / 15, frame latency 1 67 / 139 / 22, frame
latency 3 54 / 40 / 15, no vblank thread 54 / 46 / 7, simple animation driver
54 / 40 / 7, `basic` render loop 4,244 / 4,712 / 4,553. None removed the
floor; frame latency 1 made it worse.

## What others did

- **Chromium** drives its frames on Windows from DirectComposition's
  compositor clock (`DCompositionWaitForCompositorClock`) rather than from
  DWM or the output's vblank, so it wakes on the clock that actually
  composes [6].
- **Zed's GPUI** first adopted the same API with a `DwmFlush` fallback, then
  reverted to `DwmFlush` unconditionally because of bugs with
  `DCompositionWaitForCompositorClock` [7].
- **Firefox** shows the same class of problem on its DirectComposition path
  at high refresh rates (bug 1713296, tracked under a meta bug for
  high-refresh WebRender issues) [8].
- **Microsoft** recommends the waitable object for latency and notes that
  composed mode keeps a minimum of frames in flight that only independent
  flip removes; independent flip for a window needs DirectFlip or a
  multi-plane overlay plane, which DWM grants per swap chain [1].
- **Qt and KDE**: no Qt bug matches a 144 Hz composed-mode floor on Windows.
  The nearest reports are about Qt Quick throttling with `NoVSync`
  (QTBUG-99949, solved with allow-tearing) [9], animations stuttering when a
  second window opens on D3D11 [10], and KDE's QtQuick effects capped below
  144 Hz on KWin until an animation-timer fix [11]. None is this mechanism.

Phase jitter, the mechanism Chromium's compositor clock addresses, is a
small part of what we see, so copying Chromium's pacing would not remove
the floor on its own.

## Candidate fixes, ranked

1. **Keep the GPU from stretching individual frames past about 4 ms.** On
   D3D12 the remaining drops are frames whose GPU time doubles. At this load
   the RTX 4070 runs at low clocks (spike A measured the same glow frames at
   5.7 ms of GPU time at 60 Hz against 2.0 ms at 144 Hz, a sign the GPU
   clocks down at light load), and a frame that lands while the clock is
   ramping can take twice as long. Plausible fixes: the per-app NVIDIA power mode "Prefer maximum
   performance" (a user or installer setting, not an API), or less GPU work
   per frame (Qt Quick redraws the whole window each frame; nothing in Qt
   changes that). This is the most likely to work and the cheapest to test.
2. **D3D12, not D3D11.** Already the rule. It removes the buffer-starvation
   subtype (12.9 to 0.1 held drops per minute); D3D11's buffer count is
   hard-coded at 2, so there is no D3D11 knob for it.
3. **Independent flip through a multi-plane overlay.** It would move the
   deadline from DWM's composition (about 4 ms after present) to the vblank
   itself (about 6.9 ms), which covers the 4 to 7 ms frames. Qt cannot
   request it, DWM grants it per swap chain, and our window never got it.
   Whether this GPU and driver offer an overlay plane for a windowed swap
   chain is unknown.
4. **Composition-clock pacing (Chromium's approach).** It addresses the 4 %
   phase collisions, not the held frames. Low value alone, and GPUI's
   experience says the API has rough edges.
5. **Frame latency 1 or 3.** Latency 1 made D3D11 worse; on D3D12 the GPU
   already starts each frame at once, so a shallower queue only removes
   slack. Not expected to help.

## The one test (phase 4)

Run the existing D3D12 light-load fling ramp at 144 Hz / 100 %
(`ramps.run_axis(tag, "rows")`, four 8 s runs, about 2 minutes) twice: once
as now, once with the NVIDIA Control Panel's per-program power management
mode for `qtspike.exe` set to "Prefer maximum performance". Compare on-screen
drops per minute (`tools/display_drops.py`) and the GPU busy p90 of all
frames. The candidate is confirmed if drops fall from about 27 per minute to
the 0 to 2 that 60 Hz shows, and if no frame needs more than about 4 ms of GPU
time. If the drops stay, the next test is a 120 Hz run of the same ramp, to
separate "high refresh" from "this display mode".

## Sources

1. Microsoft, "For best performance, use DXGI flip model":
   https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model
2. Qt 6.11, `qrhid3d11.cpp`:
   https://github.com/qt/qtbase/blob/6.11/src/gui/rhi/qrhid3d11.cpp
3. Qt 6.11, `qrhid3d11_p.h`:
   https://github.com/qt/qtbase/blob/6.11/src/gui/rhi/qrhid3d11_p.h
4. Qt 6.11, `qrhid3d12.cpp` and `qrhid3d12_p.h`:
   https://github.com/qt/qtbase/blob/6.11/src/gui/rhi/qrhid3d12_p.h
5. Qt 6.11, `qdxgivsyncservice.cpp`:
   https://github.com/qt/qtbase/blob/6.11/src/gui/rhi/qdxgivsyncservice.cpp
6. Chromium, `vsync_thread_win_dcomp.cc`:
   https://chromium.googlesource.com/chromium/src/+/main/ui/gl/vsync_thread_win_dcomp.cc
7. Zed, "Use `DwmFlush` unconditionally for Windows vsync (#39913)":
   https://git.secluded.site/zed/commit/2d34a4aec90823017566d74c6557409580ce6688
8. Mozilla bug 1713296, glitches on a high-refresh monitor:
   https://bugzilla.mozilla.org/show_bug.cgi?id=1713296
9. QTBUG-99949, Qt Quick throttled with `NoVSync` on Windows 10:
   https://bugreports.qt.io/browse/QTBUG-99949
10. Qt forum, "Adding 2nd window causes animation stuttering in 1st window":
    https://forum.qt.io/topic/125630/adding-2nd-window-causes-animation-stuttering-in-1st-window
11. KWin merge request, "effect/offscreenquickview: update scene graph timers
    when window refreshed":
    https://invent.kde.org/plasma/kwin-x11/-/merge_requests/68
