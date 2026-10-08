# Lumen foundations

## Direction

Lumen is warm, premium, and restrained. Amber behaves as a glow or state signal, not a general-purpose fill. The interface stays dark and content-led; source identity is visible in Ristle chrome without altering provider artwork.

## Color tokens

Repeated values in the canvas establish this working palette:

| Role | Value | Usage |
| --- | --- | --- |
| Deepest surface | `#0f0c09` | Device frame and deepest background |
| Base surface | `#1b1712` | Main application surface |
| Raised surface | `#262019` | Cards, panels, and elevated regions |
| Subtle border/progress rest | `#3a342b` / `#3a322a` | Dividers, tracks, subdued outlines |
| Primary amber | `#eba24a` | Selection, active state, links, focused accents |
| Ember | `#d4632e` | Gradient endpoint and destructive swipe/action language |
| Primary text | `#f5f0e8` / `#f7f2ea` | Headlines and high-emphasis content |
| Secondary text | `#cfc6b7` / `#b8af9f` | Supporting copy |
| Muted text | `#a99e8d` / `#8a8170` / `#7d7466` | Metadata and section labels |

Rules:

- Amber fill always means **selected**. Do not use it decoratively.
- Reserve the amber-to-ember gradient for the primary play control and filled seek progress.
- Destructive actions use ember, never amber.
- Source colors identify providers; the color assigned to a provider is stable.
- Source dots live only in Ristle chrome and are never painted over album or provider artwork.

## Provider palette and source identity

The illustrated provider colors are green for Streaming A, orange for Streaming B, gray for Local, and blue for Cloud. New providers receive the next reserved color, assigned by provider and retained permanently.

Source identity can appear as a 5, 6, or 8 px dot, a labeled tag, or a cluster. Below 12 px of available label space, use a dot only. Clusters show at most three dots plus a count.

## Typography

| Role | Family | Treatment |
| --- | --- | --- |
| Headlines | Schibsted Grotesk | Bold/extra-bold, slightly tight tracking |
| UI and body | Hanken Grotesk | Default cross-platform UI family |
| Numbers only | DM Mono | Durations, timestamps, bitrates, and other numeric readouts |
| Native body override | SF Pro / Roboto / Segoe UI | Platform body text where native character matters |

Headlines remain Schibsted Grotesk on every platform. Small section labels are uppercase, semibold, and widely tracked (about `0.16em`). Keep media titles to one line where specified and ellipsize overflow.

## Shape, spacing, and elevation

The canvas repeatedly uses an approximately 4 px base grid, with common gaps and insets of 8, 12, 16, 18, and 24 px. Treat these as the working spacing scale rather than copying arbitrary one-off offsets.

- Cards and rows generally use 7–14 px radii.
- Album art uses 8–18 px radii depending on scale.
- Platform shell radii are tokens: iOS 30, Android 34, Windows 8, macOS 12.
- Mobile primary hit targets are at least 56 px; desktop compact targets are at least 44 px.
- Hover lift is subtle: cards rise 2 px; dragged rows scale to `1.015` and gain a shadow.
- Desktop row selection uses a quiet raised/tinted surface so amber remains reserved for the active or focused state; selection must remain visible when several rows are chosen.
- Opaque surfaces are the mobile default. Windows uses Mica and macOS uses vibrancy where available.

## React Native sizing and density

All dimensions in the source HTML and generated PNGs come from browser CSS pixels. They are visual targets, not a guarantee that the same numeric values will produce the same physical size in React Native.

React Native generally expresses layout in density-independent logical units, while the final device output depends on DPI, pixel ratio, OS display scaling, font scaling, and platform rendering behavior. The implementer should therefore evaluate and adjust the browser measurements rather than copying every pixel value literally.

Validate at minimum:

- Layout proportions, spacing, radii, and control sizes across representative low-, medium-, and high-density devices.
- `PixelRatio` behavior for borders, hairlines, shadows, raster artwork, and other details that can become soft or disproportionately thick.
- Text metrics and wrapping with native font rendering, user font scaling, and accessibility text sizes.
- Minimum physical touch targets independently of how closely a control matches the browser mockup.
- Image asset resolution and cache variants so artwork remains sharp without excessive memory use.
- Desktop display scaling and window resizing on React Native Windows or macOS.
- Safe areas, system bars, title chrome, and native navigation insets, which are not fully represented by the browser frame.

Compare screenshots from actual React Native targets with these references at equivalent viewport sizes. Preserve the hierarchy, proportions, and interaction intent when an exact browser-pixel translation conflicts with native usability or platform conventions.

## Artwork rules

- Never place a play overlay on artwork. The row/card or detail-page control owns playback.
- The `MIX` badge may sit on Ristle-generated mosaic covers only. On provider artwork, move it into the caption.
- Provider branding and source tags must not obscure or modify supplied artwork.
