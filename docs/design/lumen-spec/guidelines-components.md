# Lumen component guidelines

## TrackRow

Use one TrackRow everywhere: playlists, queue, albums, artist pages, search, and provider spaces. Plugins pass a track handle into the shared component; they do not supply replacement row UI.

Anatomy:

1. Optional drag handle, shown only on mobile in queue and playlist edit mode; desktop drags the row itself.
2. Leading slot: artwork, index, or nothing.
3. Mandatory source dot followed by `Artist · Album` metadata.
4. Optional metadata badge such as `FLAC`, `MIX`, explicit, or downloaded.
5. Trailing duration or a plugin slot no wider than 88 px.
6. Like and overflow actions; hover-only on desktop.

Supported states include default, hover, selected, now playing, queue upcoming/history, mobile edit, desktop/mobile dragging, numbered album, artist-popular, compact plugin, and unavailable.

Behavior:

- Mobile tap plays and long-press opens the context menu. Desktop click selects, double-click plays, and right-click opens the menu.
- Desktop lists use file-browser selection: Shift-click extends a range, Ctrl/Cmd-click toggles individual rows, and menu actions apply to the full selection.
- Plugins cannot override either behavior and may add at most two menu items.
- Title, artist, and album names are links and underline on hover. Multiple artists remain individually addressable.
- The source dot cannot be hidden.
- Desktop drag starts by click-dragging anywhere on the row and never shows a handle. Mobile uses the handle in queue/edit mode or lifts after a 250 ms long-press.
- Never add a play overlay to row artwork.
- Comfortable density is 56 px; compact density is 40 px.

## SourceTag

SourceTag is the product signature. It supports dot-only, labeled, quality-bearing, and clustered forms. Keep it in Ristle-owned metadata, list rows, captions, and control chrome—never on top of artwork.

## MediaCard

MediaCard covers playlists, albums, artists, generated mixes, and synced/read-only provider collections.

- Standard sizes: large 116 px, medium 100 px, small 88 px.
- Artist artwork is circular; other media art follows the shared rounded-square treatment.
- Titles are single-line and ellipsized.
- Captions carry a source dot followed by the provider name, then the collection's own metadata (`● SoundCloud · 22 songs`). Ristle-owned collections use amber in the dot slot and read `Ristle`.
- Desktop hover lifts 2 px and turns the title amber.
- Clicking navigates. Playback begins on the detail page or from the context menu, not from an art overlay.
- A card-level secondary action (such as Organize) is hover-only on desktop, and its slot stays reserved when hidden so a row of cards keeps one caption baseline.

## Folder tile

A playlist folder is a MediaCard whose artwork slot holds a stacked-sheet tile: two offset sheets in raised-surface tones behind a bordered face. It is the same "many items" language the multi-selection menu header uses for stacked artwork, so a folder reads as a group of collections rather than a file-system object.

- Never use a file-manager folder glyph, and never fill the tile with a provider color. Provider identity belongs to the caption's source dot, exactly as it does on any other card.
- The tile is drawn from ratios of its own size, so one motif serves the sidebar (28 px), a mobile list row (52 px), and the library grid (128 px).
- Folder captions read `Folder · N items`, counting nested folders and playlists together.
- Folders and playlists share one grid or one list — same column width, same row height, same caption block. A section is one set of collections, not two control styles.
- In a list, the folder row's disclosure chevron sits at the trailing edge so the leading tile stays aligned with the playlist rows around it.

## Chips and segmented selectors

- Filter chips allow one active value per group and scroll horizontally on mobile.
- When a chip becomes active, snap it to the left edge of the scroll area.
- Mobile chip height is 32 px; desktop height is 28 px.
- Amber fill always signals the selected value.
- Use segmented controls for exclusive setting presets such as Data Saver, High, and Lossless.

## PlayControl and transport

Use one primary PlayControl with size and platform-shape tokens:

- 64 px: Now Playing.
- 50 px: page headers.
- 42 px: desktop transport.
- 30 px: inline/feed use.
- Circle on iOS, Windows, and macOS; Android uses the M3 squircle.

Pressed controls scale to `0.96` over 120 ms. Shuffle/repeat use amber plus a dot for active state; repeat-one adds the `1` marker.

## Context menu

One action definition renders as a desktop cursor flyout or a mobile bottom sheet. Preserve this order:

1. Queue: Play next, Add to queue.
2. Collection: Add to playlist, Save to Liked Songs.
3. Navigation: Go to album, Go to artist.
4. Source-specific actions such as Open in Provider and Share, only when supported.

Both forms begin with the same source-aware track header.

A desktop multi-selection uses the same flyout with a selection-count header. Every offered action applies to all selected tracks; omit actions that only make sense for one track.

## Engagement pill

Use the shared Like and Repost pills in feed cards, provider profiles, and transport chrome. The current-track heart and TrackRow heart are the same Like action bound to the current item.

- Default pills use a quiet raised surface; active pills use amber text/icon on an amber-tinted surface.
- Mobile may use the compact bare icon/count form when space is constrained.
- Omit a pill when the source does not expose that capability; do not show a disabled substitute.
- Actions write through to the source platform where its API permits it. Update optimistically, refresh counts from the provider, and revert on failure.

## Small primitives

- **SectionHeader:** uppercase label with an optional trailing action such as “See all”.
- **Source-grouped SectionHeader:** when a list is grouped by provider, the header is a source dot, the uppercase provider label, a hairline rule filling the remaining width, and a right-aligned numeric count in the numeric family. Group headers stay this light everywhere they appear — sidebar, mobile library, desktop library. Do not give a group a filled bar, a boxed background, or a slab of provider color: the rule alone separates sections, and amber fill remains reserved for selection.
- **Toggle:** shared on/off semantics with platform-native presentation where appropriate.
- **Toast:** bottom-center on mobile, bottom-left on desktop; visible for 3 seconds; one at a time; may include an undo action.
- **EmptyState:** name a concrete next action, use warm copy, and never blame the user.
- **Back button:** one semantic component with platform tokens for glyph, shape, and material.

## Provider view primitives

Provider plugins declare views as data and never ship UI. Lumen maps result rows, social counts, supported actions, and connection requirements onto its core rows, cards, buttons, empty states, and lock states.

- A provider profile can expose tracks, reposts, likes, playlists, follows, play counts, and other supported social counts through the shared components.
- A `ViewLink` carries an opaque provider view handle for drill-down navigation; it does not carry markup or a plugin-defined component.
- When a declared view requires an account, Lumen owns the connection-required treatment and its primary Connect action.
