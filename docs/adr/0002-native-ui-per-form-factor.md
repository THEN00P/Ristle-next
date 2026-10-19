# ADR 0002: Native UI per form factor

## Status

Accepted (2026-10-08). The desktop toolkit is subject to spike A. Spike A
(2026-10-18, completed 2026-10-19): Qt Quick is a go; both conditions of the
first verdict are met (see the amendment at the end).

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

## Amendment (2026-10-18, completed 2026-10-19): spike A result

Spike A measured Qt Quick 6.11.3 with the frame-timing method
(`docs/research/qt-quick-spike.md`). **Result: go.** The first verdict was
conditional on two measurements, both run on 2026-10-19:

1. **A queue variant with no corruption under bursts and paced rates: met by
   motion-only transitions.** Queue rows appear and disappear without fading
   or scaling; the other rows slide. Across 30 duplication captures (bursts of
   2 to 5 edits 0 to 30 ms apart, paced rates of 5 to 200 per second, and the
   full queue scenario with drag reorders) the checker found nothing. The
   paced-rate captures are certified frame-complete. The burst and scenario
   captures each missed 5 to 34 of up to 8,600 desktop frames, so they are not
   certified by the method's letter; every defect the other variants produced
   lasted at least 15 frames. Every fading variant still breaks: Qt's
   documented settling pattern leaves a row missing or out of order for up to
   0.7 s a few times a minute under bursts.
2. **Memory flat over a 3-hour soak: met.** After 30 minutes the Win32 heap's
   allocated bytes grow 0.8 MB per hour, GPU memory not at all, and private
   bytes stay within 446 to 456 MB. Dropped frames stay at about 0.4 per
   minute with no upward trend once the soak's own memory sampling is
   excluded.

The trigger this ADR names (animated list edits only with Elisa-style
workarounds) has not fired: motion-only transitions are plain QML. GPUI is
an optional comparison.

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
- Animated lists whose edits can interrupt each other (the queue, anything a
  Party or sync edits) animate position only: no opacity or scale in `add`,
  `remove` or `displaced`. Qt stops a running view transition when the next
  edit arrives, and no fading variant tested (Qt's documented settling
  pattern, row-owned animations, opacity computed from the clock) came out
  clean.
- Set `NativeRendering` for text. The default distance-field text is softer
  than every native renderer at every scale.
- Keep pages alive after their first visit, and build a page that is not
  alive yet with `Loader { asynchronous: true }` behind a skeleton. That
  removes every dropped frame (27 and 77 per minute synchronous at 144 Hz) at
  the cost of 25 ms more until the page is populated.
- No `MultiEffect` per list row. `RectangularShadow` gives the same glow at no
  measurable cost.
