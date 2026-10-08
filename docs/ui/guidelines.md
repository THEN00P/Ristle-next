# UI rules

These rules apply across every platform. The visual language, Lumen (warm amber
glow on dark), is in `docs/design/lumen-spec/` and the tokens are in
`docs/design/lumen-tokens.reference.ts`. Keep the colors and the vibe. These
rules fix what Ristle legacy got wrong.

## Attribution

- Show the Provider **once per context**. A Spotify playlist says "Spotify" in
  its header, not on every row.
- Per row only in **mixed** lists (Liked Songs across Providers, search, queue),
  and then as a small Provider glyph, not dot plus name.
- One component decides attribution everywhere. Service-required attribution
  (logos, link-back) is implemented there and nowhere else.

## Chips and badges

- **At most one row of chips per screen.** Secondary filters go into a filter
  menu or sheet. Content type uses tabs, and a scope that follows from context is
  not a chip at all (inside a Party, search targets the host).
- **Badges only for states that change a decision:** explicit, now playing,
  unavailable, downloading or transferring. Not for roles (HOST), Providers or
  counts.

## Desktop is its own layout

- **Track lists are tables:** `#`, title and artist, album, date added,
  duration. Use the width. Row height is 44–48 px.
- Row actions (like, more) appear on hover, focus, and on the current row.
- The current row gets a full-width tint, not a bordered card inside the list.
- Pages use the available width. Use multi-column layouts (Party: members ·
  now playing and queue · invite) or move secondary content into popovers.
- Keyboard: every list is navigable, and core actions have shortcuts.

## Mobile

- Row height is 56–64 px. Actions go into the context sheet. The Android sheet in
  Ristle legacy is the reference for tone and density.
- Avoid repeated primary buttons in lists. One play affordance per tile at most,
  and only where playing is the main action.

## Never empty, never technical

- Collections without artwork get a **generated cover** (Lumen gradient plus
  glyph). Loading uses skeletons. An empty box is a bug.
- No internal values in the UI: "21+", "ALL PROVIDERS", plugin ids, raw errors.
- Feedback ("link copied", "added to queue") is a toast, never loose text in the
  layout.
- Actions live in toolbars, headers or context menus, never as stray text links
  below content.

## Motion

- Page switches show the next page immediately with cached data and fill in the
  rest. Navigation never waits on the network.
- Queue insert, remove and reorder are animated on every platform.
