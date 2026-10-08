# Ristle design specification — Lumen

This directory translates the original visual canvas into an implementation-oriented reference. The source of truth remains [`Ristle - Lumen.dc.html`](./Ristle%20-%20Lumen.dc.html); the Markdown files make its rules easier to scan and review.

## Guides

- [Foundations](./guidelines-foundations.md) — direction, color, typography, shape, elevation, and selection semantics.
- [Components](./guidelines-components.md) — TrackRow, SourceTag, MediaCard, chips, playback controls, menus, and small primitives.
- [Platforms and interaction](./guidelines-platforms-and-interaction.md) — shared behavior, platform tokens, navigation, motion, gestures, and menus.
- [Page patterns](./guidelines-page-patterns.md) — mobile, desktop, search, library, playback, provider spaces, and settings.
- [Generated page gallery](./generated-pages/README.md) — rendered PNGs for every labeled page.

## Implementation boundary

The designs describe one product and one behavior model. In line with the repository ownership rules, shared orchestration, playback, queue, persistence, provider, cache, and validation behavior belongs in `packages/app-core` (with domain/storage primitives in `packages/core`). Platform code should only adapt native UI chrome, OS lifecycle and permissions, filesystem access, media sessions, credential storage, and authorization UI.

When two targets share behavior, implement it once in the shared core and expose only the smallest platform adapter needed.

## React Native dimension caveat

The measurements in the HTML and generated PNGs are browser-rendered CSS pixels, not guaranteed one-to-one React Native dimensions. Treat them as visual design references rather than literal device-pixel specifications. During implementation, evaluate how React Native density-independent layout units map across device DPI and pixel densities, and verify the result at representative iOS, Android, Windows, and macOS scale factors. See [Foundations → React Native sizing and density](./guidelines-foundations.md#react-native-sizing-and-density) for the implementation checklist.
