# Page pattern guidelines

## Mobile shell

The mobile family shares a single component tree with small OS chrome differences. Home, Feed, Search, and Library form the primary navigation. A persistent mini-player sits above the navigation and carries artwork, title, source, progress, and compact controls.

## Home

- Lead with a time-aware greeting and profile affordance.
- “Jump back in” uses compact resumable rows.
- “Made for you” uses media cards with visible mixed-source identity.
- Provider spaces surface as Home cards rather than new global navigation items.

## Playlist and album detail

- Use artwork, type label, title, ownership/count/duration metadata, and a prominent 50 px PlayControl.
- Tracks use the shared TrackRow and always show source identity.
- Mixed collections use Ristle-owned `MIX` treatment; synced provider collections remain read-only where required.

## Now Playing and queue

- Keep the source tag below the artwork in owned chrome.
- Show title, artist/album, quality, seek state, and transport as a stable shared layout.
- The queue separates Now Playing, Next Up, and dimmed history states.
- The desktop queue docks as an inset panel on the right side of any page and is toggled from the transport bar. It participates in the desktop layout, shifting the main content rather than overlapping it.
- The mini-player is the same conceptual component on mobile and desktop; desktop may pin it above other windows and reveal Up Next on hover.

## Search

- Search queries span every source.
- Empty search shows browse categories and recent items.
- Results use type chips, a Best Match block, and grouped Songs, Albums, and Artists with “See all” actions.
- Desktop song results include an album column; mobile compresses the same information into TrackRow metadata.

## Library

- Distinguish Ristle mixes from provider-synced playlists.
- Use tabs/filters for Playlists, Albums, and Local content.
- “Liked Songs” is an automatic cross-source playlist.
- Local-file downloads/offline state belongs in playlist controls and Library filters, not a new navigation destination. Streaming downloads are post-MVP.

### Grouping and folders

- The Playlists filter groups collections by source, Ristle-owned first and connected providers after it in name order. Each group opens with the source-grouped SectionHeader and states its own count.
- Folders and playlists appear together in one grid per group, folders first. A folder uses the folder tile; nothing else distinguishes it from the playlists beside it.
- Opening a folder replaces the grouped view with that folder's contents and puts a breadcrumb back to its parent in the page's section header.
- Folder management lives in the section header's trailing action slot — New folder alongside the count, Delete folder only inside a Ristle-owned folder, in ember. Do not stack management buttons in a band under the header where they compete with the filter chips.
- Provider folders mirror what the provider stores and are read-only. Only Ristle-owned folders accept new folders, deletion, or a moved playlist.
- An empty folder gets an EmptyState naming the move action, not a bare blank grid.

The same rules hold on mobile with rows instead of cards: the grouped header, folder rows carrying the folder tile and a `Folder · N items` caption, and folder actions as chip-height pills on the filter row's baseline.

## Artist pages

Core artist content includes identity, listener/source summary, Follow, Shuffle/Play, a numbered Popular top five with a Show all action, discography filters, and related artists. Optional provider-backed modules include playlists featuring the artist, provider profile content, liked tracks, reposts, and fan listening. Hide unsupported modules cleanly rather than showing empty scaffolding.

## Feed

- Feed is a top-level destination on mobile and desktop and merges activity from every connected provider.
- Every entry keeps its SourceTag. Provider spaces remain available for views that cannot be merged.
- Like and repost pills write through to the source when supported and refresh their counts from it. Omit unsupported actions instead of disabling them.
- New-release alerts live here and in provider spaces; Liked Songs remains the cross-source automatic playlist in Library.

## Provider spaces

Provider pages render inside Lumen chrome with shared primitives. The plugin declares a data view; Lumen chooses and renders the rows, cards, social elements, controls, loading treatment, and connection-required state. Feed entries may expose source-native activity and “Add to mix” without introducing plugin-owned UI.

A provider `ViewLink` can open a profile or another drill-down using an opaque view handle. The destination remains a Lumen-rendered surface and may expose tracks, reposts, likes, playlists, follows, play counts, or other provider-supported social data.

## Add and connect a plugin

- Group connector candidates by provider and rank maintained entries by cached GitHub stars. Display archived and last-update metadata without making a live GitHub request.
- “Install from file” accepts a `.ristleplugin` archive only after every file matches its checksum manifest; verify before staging.
- Continue account connection in the system browser through an authorization session. Ristle and the plugin never receive the user's password; the plugin receives tokens after the browser flow completes.
- Show waiting, connected, unfinished, and expired states with clear Cancel, Try again, or Reconnect actions.
- A view that requires a connection uses the shared lock state and Connect action. When disconnected, keep cached provider content browsable and mark unavailable tracks with the shared row state.

## Settings

- Settings is one scrolling page ordered General, Playback, Library, then Providers; it has no secondary navigation.
- Present one global streaming-quality preset; each provider maps it to the closest supported tier.
- Allow explicit per-provider overrides.
- Expand a provider row in place for account, quality override, sync, and storage controls.
- Group playback, output, library, and provider account controls within their page sections.
- Crossfade, output selection, local folder import, provider management, and manual per-provider volume trim fit existing Settings groups. Gapless transition and Playback Session restoration are unconditional behavior, not settings.

## Desktop shell

- The left pane orders Home, Search, Feed, and Your Library above the user's playlists. Each destination carries its own glyph; Feed uses the broadcast mark, never the heart, which stays bound to the Like action.
- Show Ristle mixes with the `MIX` marker and synced playlists with their stable source dot.
- “New playlist…” belongs with the playlist list. Settings stays anchored at the bottom of the pane.
- The playlist list scrolls independently under the destinations and is grouped by source with the source-grouped SectionHeader, so the sidebar and the Library page describe the same shape.
- Sidebar rows are two-line: a 28 px leading tile (artwork for a playlist, the folder tile for a folder), the title, and a metadata line — track count for a playlist, `N items` for a folder. Both row kinds share one height and one text column.
- A folder expands in place. Nested rows indent by one step; the disclosure chevron stays at the trailing edge so every leading tile in the list stays on one vertical line.
- Selection uses the same amber rail and tinted surface as the destinations above, and only one row in the pane is selected at a time.

## Existing homes for remaining player features

| Capability | Surface |
| --- | --- |
| History / recently played | Home and Library |
| Downloads and offline | Local files in MVP; streaming post-MVP |
| Output device and cast | Transport and Now Playing sheet |
| Sleep timer | Now Playing overflow |
| Lyrics | Now Playing swipe-up, when supported |
| Local folder import and scan | Settings → Local Files |
| Local metadata editor | Local track context menu |
| New-release alerts | Feed and provider spaces |
| Per-provider volume handling | Settings → manual per-provider trim |
