# ADR 0002: Native UI per form factor

## Status

Accepted (2026-10-08). Accepted for desktop (2026-10-19): Qt Quick is the
desktop toolkit, React Native and GPUI are rejected (see "Amendment
(2026-10-19): desktop decision" at the end). Spike A certified Qt Quick at
60 Hz; the 144 Hz hiccups are being investigated
(`docs/research/144hz-hiccups.md`) and do not block.

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
  carries, re-evaluate GPUI before building the desktop app. (Superseded: the
  trigger did not fire, and the user rejected GPUI on 2026-10-19.)

## Amendment (2026-10-18, completed 2026-10-19): spike A result

Spike A measured Qt Quick 6.11.3 with the frame-timing method
(`docs/research/qt-quick-spike.md`). **Result: go, for the conditions
measured.** It passes at 60 Hz (100 to 150 % with D3D11, 100 % with D3D12)
and on the discrete GPU. The first verdict was conditional on two
measurements, both run on 2026-10-19:

1. **A queue variant with no corruption under bursts and paced rates: met by
   motion-only transitions.** Queue rows appear and disappear without fading
   or scaling; the other rows slide. Across 29 duplication captures (bursts of
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
   bytes stay within 446 to 456 MB. Dropped frames on screen stay at 12
   per minute with no upward trend.

**Correction (final review round): dropped frames are counted on the
screen** (PresentMon's `MsBetweenDisplayChange`), not between presents. That
flips two earlier claims. At 144 Hz Qt Quick does not run drop-free: every
run, at any load, drops 1 to 9 single frames per 8 s on screen (about 12 to
43 per minute) on both backends, while the app presents on schedule. Load
does not raise the rate until knees far above the production envelope, but
the method's knee cannot be shown below this floor, and whether it is Qt's
or the compositor's is not attributed. At 60 Hz nothing changes: no frame
drops on screen at light load. And D3D12 is no longer the clear win it
seemed: on screen it is equal to D3D11 at 100 and 125 % light load and better
at 150 % (13 against 43 per minute) and in the 30-minute soak (21 against 36),
not fifty times better.

Not measured: D3D12 at 60 Hz with 125 and 150 % scaling (D3D11 passed there,
and D3D12 passed all three scales at 144 Hz). Phase 4 checks it, and
attributes the 144 Hz floor with a minimal native flip-model swap chain at
144 Hz on the same machine.

The trigger this ADR names (animated list edits only with Elisa-style
workarounds) has not fired: motion-only transitions are plain QML. GPUI is
an optional comparison.

Decisions taken with the user on 2026-10-18: software rendering only has to
stay usable (no freeze of 100 ms or more), and slow fresh-page creation at
144 Hz is not a deal breaker. Both were failing criteria in the first version
of the report.

What passed on the discrete GPU, with no drops on screen at 60 Hz and none
above the floor at 144 Hz: scrolling up to 40M rows, model churn up to 1,000
edits per second, flings at Qt's velocity cap, wheel and touchpad, uncached
artwork, one CPU core with the power-saver plan, sidebar toggles over 1M
rows, cached page switches and the MultiEffect glow; soaks of 30 minutes to
3 hours with a flat drop rate; and text that matches GDI ClearType.

Rules for the Qt Quick app, replacing and extending the ones above:

- Use the D3D12 backend. On screen at 144 Hz it drops fewer frames than
  D3D11, Qt's default, at 150 % and over long runs (21 against 36 per minute
  in the 30-minute soak), and the same at 100 and 125 %. It costs about
  160 MB more memory. Phase 4 may revisit this once the 144 Hz floor is
  attributed.
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
  removes the frames the page build drops (on screen at 144 Hz, 55 down to 11
  per minute with heavy rows) at the cost of 25 ms more until the page is
  populated.
- No `MultiEffect` per list row. `RectangularShadow` gives the same glow at no
  measurable cost.

## Amendment (2026-10-19): desktop decision

Decisions the user took after spike A:

- **React Native is out for good**, on desktop and as a fallback. The user
  found it bad all round in Ristle legacy: transparency, mouse events, big
  lists, gradients and more.
- **GPUI is rejected.** The user tried it hands-on: scrolling behaved oddly
  and its antialiasing felt alien next to native Windows text. It is no
  longer an optional comparison, and spike A's harness is not kept for it.
- **Qt Quick is the desktop toolkit**, with the four settings (D3D12, no
  `Animator` types, position-only list transitions, `NativeRendering` text)
  and the phase 4 rules in the spike A amendment above.
- **The 144 Hz hiccups are to be investigated and ideally removed, but they
  do not block.** `docs/research/144hz-hiccups.md` traces them in the
  recorded data: 93 % are frames whose GPU work finishes after DWM's
  composition deadline, not two presents colliding in one refresh. It names
  one short phase 4 test (the GPU power mode) for the most likely fix.
