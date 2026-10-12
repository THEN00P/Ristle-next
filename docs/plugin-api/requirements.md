# Plugin API: requirements

Input for the WIT design (ADR 0003). This file collects what the API must cover.
The WIT itself is the next design step (see `HANDOFF.md`).

## Principles

1. **Ristle is a UI driven by Plugin data.** Everything visible except Local
   Files, Ristle Playlists and settings comes from a Plugin. The capability
   surface is therefore wide on purpose.
2. **A verbose API is worse than setup friction.** Templates, an SDK and a
   step-by-step guide can absorb setup. Every call a Plugin author has to make
   should do one obvious thing.
3. **The host does the hard generic work:** caching, paging, batching, rate
   limiting, retries, decoding and decryption. Plugins describe and the host
   executes.
4. **No ambient authority.** Every import is checked against the manifest and
   the user's consent.

## Data domains

The API must provide data for:

| Domain | Notes |
|---|---|
| Search | Typed result groups (Tracks, Albums, Artists, Playlists, Profiles). The host assembles the Unified Search snapshot |
| Tracks | Exact identity, capabilities (seekable, live), Media Dates, Metadata Attributes, availability |
| Albums, Artists | Artist pages with releases and profile modules |
| Profiles | Users and creators (SoundCloud-style) |
| Playlists | Public and private, owned and followed, plus write operations |
| Feeds | One Feed merged across Providers from typed posts. Provider Views only for Provider-specific surfaces |
| Likes | Read liked state (batched), like and unlike, liked collections |
| Streams | `resolve(track) → StreamSource`, see ADR 0004 |

## Collections, paging and lazy loading

Lessons from Ristle legacy that the API must encode:

- **Cursor paging with an unknown total.** A total is optional, and an estimate
  may be marked as one.
- **Positional handles.** Some services address entries only by position
  (Spotify Liked Songs: `offset=N`). The API must say whether a handle is a
  *stable id* or a *position that drifts* when the collection changes, so the host
  never silently maps a position onto a neighbor.
- **Lazy hydration.** A collection can enumerate cheap handles first, and the
  host hydrates them into Tracks in batches. The batch size is declared by the
  Plugin (Spotify allows 50 per request).
- **Batched liked state.** One call answers many Tracks, never one call per row.
- **Change tokens.** A Plugin can return an etag or snapshot id so the host can
  revalidate cheaply.

## Host services (imports)

| Service | Requirements |
|---|---|
| HTTP | HTTPS only, hosts declared in the manifest, bounded size and time, concurrency limit per Plugin, redirects allowed only within declared hosts |
| HTTP cache | The host honors `Cache-Control` and lets the Plugin override TTLs and set cache keys per request. Stale-while-revalidate for browse data |
| Rate limiting | The host parses `Retry-After` and 429/503, backs the Plugin off, and reports it in the UI. Plugins never sleep-loop |
| Storage | Per-Plugin key-value store with a quota, and a separate encrypted Credential Record per Provider Connection |
| Time, randomness, logging | Standard, with a bounded log volume |
| Crypto | AES, HMAC, SHA and base64 implemented natively, so interpreted Plugins on iOS do not pay for them |

## Sign-in (Authorization Sessions)

All of these must be expressible without Plugin-specific host code:

1. **OAuth 2 with PKCE.** Redirect to a loopback port (desktop), a custom scheme
   or app link (mobile), or a remote HTTPS redirect page. The host owns `state`
   and the callback, and only the code reaches the Plugin.
2. **Deep link callback** from the service's own app.
3. **API token or credential form**: fields declared by the Plugin and rendered
   by the host.
4. **Web capture**: an embedded web view on the service's real site, with
   declared rules for which requests, headers, cookies or responses to capture
   (domain allowlist, header names, a completion predicate). This is what the
   SoundCloud client-id and token capture needs. A desktop user agent must be
   selectable, because the mobile site can be a different application.
5. **Re-authentication.** A Plugin can report expired credentials, and the host
   prompts the user without losing the Provider Connection.

## Packaging and trust

- **One file.** A Plugin Package is a single `.wasm` component. A WASM file is
  not an archive, but it can carry **custom sections**: put the manifest (id,
  name, description, author, version, requested hosts and capabilities) and the
  icon there. No zip needed.
- **Signature in a custom section** over the rest of the module. Prior art:
  `wasmsign2`, which signs WASM modules this way.
- **Publisher continuity**, mirroring Ristle legacy: an update must be signed by
  the installed Publisher key. A different key is a **takeover**. It is
  possible, but the user must accept it explicitly with a clear warning. A
  handover signed by the old key softens the warning but does not remove it.
- **Permission diffs on update.** New hosts or capabilities require consent
  again.

## Open questions for the WIT draft

The draft's proposed answers are in `walkthrough.md`, under "Decisions and
open questions for the user".

1. Do Provider Views stay as plugin-defined rows and links, or does the API need
   richer layout primitives (shelves, hero, grids)?
2. How should writes report partial failure, for example when adding 50 Tracks
   to a playlist?
3. Do Plugins get background tasks, such as feed refresh or notifications, and
   if so, on what schedule budget?
4. Versioning: WIT package versions plus capability flags. What is the deprecation
   policy once third parties ship?
5. Party: which Track metadata may a Plugin mark as public for relay to guests,
   and which must stay private?
