# ADR 0002: Native UI per form factor

## Status

Accepted (2026-10-08). The desktop toolkit is subject to spike A.

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
