# ADR 0002: Native UI per form factor

## Status

Accepted (2026-10-08). The desktop toolkit is subject to spike A. Spike A
(2026-10-18): Qt Quick is a conditional go, pending the queue fix and the
3-hour soak (see the amendment at the end).

## Context

Ristle legacy used React Native on Android, Windows and macOS. Desktop ran on
out-of-tree RN platforms with renderer bugs that needed permanent workarounds,
and mobile and desktop layouts were separate anyway. JavaScript was idle in the
background, memory use was high, and page switches froze while React mounted the
next screen on the JS thread. Sharing UI code across desktop and mobile caused
more problems than it solved.

## Decision

Share UI by form factor, never across it.

| Target | UI |
|---|---|
| Android | Kotlin, Jetpack Compose, Media3 `MediaSessionService` |
| iOS | Swift: UIKit shell with `UICollectionView` for long lists; SwiftUI for leaf screens such as settings, sign-in and Party |
| Windows + macOS | One Qt Quick (QML) app. Rust models exposed through cxx-qt. Custom Lumen components, not the stock Qt Quick Controls look |

Rules for Qt Quick:

- QML holds no logic (see `AGENTS.md`). Bindings and handlers run on the GUI
  thread, and so do scrolling and ordinary animations.
- Use `Animator` types for motion that must survive a busy GUI thread.
- Lists use `reuseItems` and slim delegates.

## Consequences

- Three UI codebases. Each is idiomatic, and each can be profiled with its
  platform's own tools.
- Desktop shares one codebase across Windows and macOS. Media keys and the
  system now-playing integration need a small platform shim (SMTC on Windows,
  `MPRemoteCommandCenter` on macOS).
- Qt is used under LGPLv3 with dynamic linking (see ADR 0009). Avoid GPL-only
  add-on modules unless the licence decision allows it.
- If spike A shows that animated list insert, remove and reorder, or Windows
  text sharpness, can only be achieved with workarounds of the kind Elisa
  carries, re-evaluate GPUI before building the desktop app.

## Amendment (2026-10-18): spike A result

Spike A measured Qt Quick 6.11.3 with the frame-timing method
(`docs/research/qt-quick-spike.md`). **Result: go, conditional.** Qt Quick
stays the desktop toolkit if the measurement batch of 2026-10-19
(`spikes/a-qt-quick/tools/monday_batch.py`) settles two conditions:

1. A queue transition variant with no corruption under bursts of edits a few
   milliseconds apart and under paced rates up to 200 per second. Qt's
   documented settling pattern still leaves a row half-faded after three edits
   30 ms apart; two variants that cannot leave a row half-faded are ready.
2. Memory flat over a 3-hour soak, or growth that the logged heap and GPU
   figures show to be caches that stop growing.

If no queue variant comes out clean, the trigger this ADR names (animated list
edits only with Elisa-style workarounds) has fired, and GPUI is evaluated with
the same harness. Otherwise GPUI is an optional comparison.

Decisions taken with the user on 2026-10-18: software rendering only has to
stay usable (no freeze of 100 ms or more), and slow fresh-page creation at
144 Hz is not a deal breaker. Both were failing criteria in the first version
of the report.

What passed, on the discrete GPU at 60 and 144 Hz and 100 to 150 % scaling:
scrolling up to 40M rows, model churn up to 1,000 edits per second, flings at
Qt's velocity cap, wheel and touchpad, uncached artwork, one CPU core with the
power-saver plan, sidebar toggles over 1M rows, cached page switches, the
MultiEffect glow, a 60-minute soak with 25 dropped frames, and text that
matches GDI ClearType.

Rules for the Qt Quick app, replacing and extending the ones above:

- Use the D3D12 backend. D3D11, Qt's default, drops 20 to 50 frames per
  minute at 144 Hz. D3D12 costs about 160 MB more memory.
- **Do not use `Animator` types.** This reverses the rule above. A running
  Animator makes every GUI-thread frame span two vsyncs on all flip-model
  backends, so every other animation runs at half rate.
- Queue and other animated lists must never leave a row half-faded. Use the
  variant the 2026-10-19 batch confirms; until then, at least Qt's documented
  handling of interrupted transitions (drive opacity and scale back to 1 in
  `displaced`).
- Set `NativeRendering` for text. The default distance-field text is softer
  than every native renderer at every scale.
- Keep pages alive after their first visit, or load them asynchronously
  behind a skeleton (decided by the batch).
- No `MultiEffect` per list row. `RectangularShadow` gives the same glow at no
  measurable cost.
