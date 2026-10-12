# ADR 0012: Respect Provider terms where the design allows

## Status

Accepted (2026-10-12)

## Context

Spotify's Developer Terms and Developer Policy (read 2026-10-12) restrict what
an application may do with Spotify content:

- Content may only be displayed back to the application's own user and must
  not be transferred to third parties (Terms IV.2.5, V.4).
- No playback of one source to several simultaneous listeners (Policy III.4).
- No product integrated with streams or content from another service
  (Policy III.5), and no segueing, mixing or overlapping Spotify content with
  other audio (Policy III.7).
- Displayed content is attributed to Spotify and links back to it (Policy
  II.4.1, II.4.2). Content is not stored indefinitely (Terms IV.3.1).

Ristle plays several Providers in one queue and plays to a room in a Ristle
Party. The Spotify Plugin uses a playback client rather than an approved
developer application, and lives outside the app repository for that reason
(ADR 0010). Other Providers will have terms of their own.

## Decision

Ristle follows Provider terms wherever its design allows. These measures are
good faith, not a legal position: a Provider that wants to stop Ristle can
do so whatever Ristle does.

Ristle's reading of the Spotify rules, which the measures below implement:

- **Mixing (III.5, III.7)** means blending audio or passing one service off as
  another. A queue that holds Spotify Tracks next to other Providers' Tracks
  does not mix them when the audio never overlaps and the Provider of every
  Track is clearly shown. So: no crossfade or other overlap into or out of a
  Spotify Track, and Spotify Tracks carry the Provider's icon or a text badge,
  as every Provider's Tracks do. That badge is the attribution; no extra
  attribution line or link is added.
- **Third parties (IV.2.5)** are kept from Spotify metadata in a Ristle Party
  by relaying only the Track reference. A guest whose own Ristle has a Spotify
  Provider Connection resolves the metadata with their own account. Any other
  guest sees that a Spotify Track is queued, without its metadata.

These are generic mechanisms in the host, switched on by the Plugin's
manifest, never Spotify-specific host code:

- `[party] share` lists the fields a Party Host may relay. An empty list
  relays only the Track reference.
- `[playback] overlap = false` forbids crossfades and any other overlap at the
  boundaries of that Provider's Tracks.

## Consequences

- Party guests without a Spotify connection see placeholders for Spotify
  Tracks. That is a deliberate cost.
- A future crossfade feature has to honour `overlap = false` from its first
  version.
- Plugins for other Providers declare their own restrictions the same way,
  and the reading above is the template for interpreting similar terms.
