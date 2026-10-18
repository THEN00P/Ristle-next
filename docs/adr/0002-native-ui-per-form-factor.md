# ADR 0002: Native UI per form factor

## Status

Accepted (2026-10-08). The desktop toolkit is subject to spike A. Spike A
(2026-10-18) was a no-go against the frame-timing method; GPUI is evaluated
next (see the amendment at the end).

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
(`docs/research/qt-quick-spike.md`). **Result: no-go as the method defines
it, so GPUI is evaluated with the same harness before phase 4.** The desktop
row of the table above stays provisional until that comparison is done and
reviewed with the user.

What failed:

- No software rendering path is smooth. WARP, Mesa llvmpipe and the Qt Quick
  software adaptation all drop frames scrolling 1,000 rows with artwork,
  because Qt Quick redraws the whole window every frame.
- Building a page that is not cached costs 3 to 5 frames at 144 Hz, so the
  first visit to a page sits at its knee instead of 4× below it.
- Memory was still creeping after an hour of soak (3 MB in the last 35
  minutes), so "flat after warm-up" is not met. The cause is not known.

What passed, on the discrete GPU at 60 and 144 Hz and 100 to 150 % scaling:
scrolling up to 40M rows, model churn up to 1,000 edits per second, flings at
Qt's velocity cap, wheel and touchpad, uncached artwork, one CPU core with the
power-saver plan, sidebar toggles over 1M rows, cached page switches, the
MultiEffect glow, a 30-minute soak with 13 dropped frames, and text that
matches GDI ClearType. The trigger this ADR named (animated list edits or text
sharpness only with Elisa-style workarounds) did not fire.

If Qt Quick is chosen, these rules replace and extend the ones above:

- Use the D3D12 backend. D3D11, Qt's default, drops 20 to 50 frames per
  minute at 144 Hz. D3D12 costs about 160 MB more memory.
- **Do not use `Animator` types.** This reverses the rule above. A running
  Animator makes every GUI-thread frame span two vsyncs on all flip-model
  backends, so every other animation runs at half rate.
- List view transitions drive opacity and scale back to 1 in `displaced`
  (Qt's documented handling of interrupted transitions). Without it, queue
  edits from 5 per second leave rows dimmed, overlapping or missing for good.
- Set `NativeRendering` for text. The default distance-field text is softer
  than every native renderer at every scale.
- Keep pages alive after their first visit, or build them with asynchronous
  delegate incubation.
- No `MultiEffect` per list row. `RectangularShadow` gives the same glow at no
  measurable cost.
