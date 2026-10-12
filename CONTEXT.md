# Ristle

Ristle is a unified listening application that presents music from independently integrated sources through one playback experience.

## Language

**Provider**:
A music source that Ristle can search, browse, or play, such as Spotify, SoundCloud, or a local library.
_Avoid_: Service, platform

**Provider Plugin**:
An installable Plugin that integrates one Provider with Ristle.
_Avoid_: Provider, adapter

**Provider Connection**:
A user's configured access to a Provider, including an authenticated account or selected local library. Provider-owned playlists, saves, uploads, and writes retain their specific Provider Connection; ordinary catalog Tracks do not.
_Avoid_: Provider, account

**Credential Record**:
Provider Connection authentication data containing a versioned auth scheme, standard token or login fields, and optional encrypted custom data. Plugin access grants and callback configuration are stored separately.
_Avoid_: Plugin credentials, auth configuration

**Authorization Session**:
A short-lived authentication attempt started for one Plugin and one pending Provider Connection. Ristle owns its opaque state and callback route, opens the URL prepared by the Plugin, validates the response, and returns only the authorization code to that same Plugin for token exchange. Plugin identity and credentials never need to appear in the callback URL.
_Avoid_: Login session, callback token

**Track**:
The exact Spotify item, SoundCloud upload, Local Track, radio stream, or other Provider item selected by the user. Each Track declares playback capabilities such as seeking; another copy of the same song from a different Provider is a different Track and is never substituted automatically.
_Avoid_: Abstract song, interchangeable recording

**Live Track**:
A Track with an unknown or advancing end time, such as a live YouTube or radio stream. It may be non-seekable and cannot rely on end-of-Track transition preparation.
_Avoid_: Infinite song, radio-only item

**Disabled Track**:
A Track that is known to be unavailable or whose required Provider Plugin is disabled or absent. It retains cached title, artist, Provider attribution, and available artwork, but cannot enter the Canonical Queue and is silently excluded when a collection is queued; catalog searches may omit unavailable Tracks entirely.
_Avoid_: Hidden track, delayed track

**Local Track**:
A Track identified from its audio content rather than a file path. Duplicate files remain hidden playback candidates; Ristle prefers FLAC over MP3, then higher bitrate within the same format, and falls back to another duplicate when necessary.
_Avoid_: Local file, path-based track

**Ristle Playlist**:
A playlist owned by Ristle that may contain Tracks from multiple Providers. It is independent from playlists owned by a Provider.
_Avoid_: Unified playlist, provider playlist

**Provider Playlist**:
A playlist owned by one Provider that may contain only Tracks from that Provider.
_Avoid_: Ristle Playlist, imported playlist

**Playlist Add**:
An operation that appends Tracks from a Track, album, or playlist to a compatible destination playlist. When exact Track duplicates exist, the user chooses whether to add all Tracks or only new ones.
_Avoid_: Import, synchronization

**Metadata Attribute**:
An optional Provider-supplied fact such as mood, danceability, or audio quality that Ristle may display but does not require every Provider to supply.
_Avoid_: Required metadata, plugin-private data

**Media Date**:
A release, upload, playlist creation, or playlist update date with its original precision preserved as year, year-month, or full date.
_Avoid_: Timestamp, invented full date

**Unified Search**:
A search across enabled Provider Connections that keeps every matching item separate, groups results as Tracks, Albums, Artists, and Playlists, and interleaves Providers within each group using comparable relevance bands. Results appear as one stable snapshot after every Provider responds or five seconds elapse; late responses never reorder that snapshot.
_Avoid_: Merged catalog, deduplicated search

**Everywhere Search**:
Unified Search across the full catalogs exposed by enabled Provider Connections.

Search presentation may conservatively group equivalent recordings after the
stable snapshot is assembled. A group retains every exact Provider Track and
its source identity; it never substitutes one Provider Track for another during
playback, queueing, playlist writes, or history. Authoritative identifiers are
preferred, and ambiguous metadata matches remain separate (ADR 0022).
_Avoid_: Global search, catalog mode

**My Library Search**:
A Provider-informed search and browsing scope for the user's music and collections, including saved tracks and albums, indexed Local Tracks, user uploads, owned or saved playlists, and provider-defined entries such as liked tracks or a feed. It is a navigation concept rather than a rigid ownership rule.
_Avoid_: Personal search, local search

**Plugin**:
An installable component that adds a Provider integration or another capability to Ristle.
_Avoid_: Extension, add-on

**Plugin Package**:
One `.wasm` component file. Its custom sections carry the manifest, icon and
Publisher signature.
_Avoid_: Plugin archive, bundle

**Publisher**:
The signing identity behind a Plugin Package. Updates must be signed by the same
Publisher. A change of Publisher is an explicit, warned takeover, never a silent
update.
_Avoid_: Author, developer account

**Plugin Permission**:
An explicit user grant allowing a Plugin to access credentials or another protected resource stored by Ristle. The compact consent screen names those Ristle-managed resources without repeating permissions already shown by the Provider's authorization flow.
_Avoid_: Shared access, implicit access

**Feed**:
One timeline of what the accounts a user follows published or reposted, merged by date across every Provider Connection that has one. Posts keep their Provider, author and reposter. It refreshes only when the user asks.
_Avoid_: Provider feed, stream

**Provider View**:
A browse surface only one Provider has, such as charts or a curated hub. The Plugin supplies sections of standard items and drill-down links; Ristle renders them with its standard list UI. Albums, artists, users, playlists, the library and the Feed are not Provider Views: they are typed pages laid out the same way for every Provider.
_Avoid_: Custom page, plugin UI

**Stream Source**:
How a Plugin tells Ristle where a Track's encoded audio comes from: a
progressive HTTP resource, an HLS playlist, or a Plugin Byte Source. Ristle
fetches, demuxes and decodes it. Plugins never deliver decoded PCM (ADR 0004).
_Avoid_: Plugin decoder, audio URL

**Plugin Byte Source**:
A Stream Source whose bytes the Plugin supplies on demand, by offset, for
services whose media cannot be fetched with plain HTTP, such as chunked or
encrypted CDNs. Ristle still does the decoding.
_Avoid_: PCM delivery, custom decoder

**Gapless Transition**:
A track change at the intended audio boundary with no silence introduced by Ristle, including transitions between tracks designed to play continuously.
_Avoid_: Fast transition, crossfade

**Playback Notice**:
A non-interactive message shown above the seek bar when playback is delayed or interrupted. The user continues to use the standard transport controls.
_Avoid_: Error dialog, playback prompt

**Canonical Queue**:
The authoritative ordered sequence of tracks that Ristle intends to play across all Providers.
_Avoid_: Master queue, provider queue

**Ristle Party**:
A small, physically co-located group collaboratively controlling one host
device's Canonical Queue and speaker output. Guests send requests; guest devices
do not play synchronized audio.
_Avoid_: Listening room, broadcast, shared account

**Party Host**:
The sole Ristle Party device that owns playback, applies Party Commands to the
Canonical Queue, and publishes Party State. The Relay is never the authority.
_Avoid_: DJ server, room owner

**Party Command**:
A guest's request to change the host's queue or transport, numbered with a
per-member sequence. The host applies each sequence number at most once.
_Avoid_: Message, event

**Party State**:
The host's revisioned snapshot of what guests see: a bounded queue window,
transport, and per-member acknowledgements and rejections of Party Commands. A
command's outcome is always read from Party State, never from a separate reply.
_Avoid_: Queue projection, sync payload

**Party Invite**:
An unguessable, expiring, revocable capability that lets a device join one
Ristle Party. Redeeming it yields a Guest Token unless the device is signed in.
It never contains Provider credentials.
_Avoid_: Room password, login token

**File Share**:
A guest's local audio file sent to the Party Host over an encrypted
peer-to-peer data channel, relayed through TURN only when no direct route
exists.
_Avoid_: Upload, stream

**Playback Session**:
The persisted Canonical Queue, current Track, playhead, repeat state, shuffle state, and playback intent. After a cold restart it is restored paused rather than autoplaying.
_Avoid_: Player state, listening session

**Recently Played**:
The latest 100 Tracks played locally for at least 30 seconds or half their duration, whichever occurs first. Live Tracks qualify after 30 seconds.
_Avoid_: Provider history, recommendations

**Queue Projection**:
A Provider-specific representation of the Canonical Queue that uses Projection Markers, or otherwise omits tracks, when the Provider cannot represent them.
_Avoid_: Synced queue, mirror queue

**Queue Mutation**:
A requested change to the Canonical Queue, whether initiated in Ristle or received through a Queue Projection.
_Avoid_: Queue rewrite, direct edit

**Queued Request**:
A Track or ordered group explicitly added with Add to queue. Queued Requests play after the current Track in first-in, first-out order, before playback returns to the underlying playlist order.
_Avoid_: Playlist insertion, play next

**Projection Marker**:
A clearly named Provider track used only to preserve the position of an otherwise unrepresentable Canonical Queue entry. Its audio is never intended to play.
_Avoid_: Fallback track, replacement song

**Account**:
A Ristle sign-in identity held by `ristle-server`. It is required for every
server feature that consumes quota: hosting a Party, TURN relaying, and later
shared playlists. Local listening and joining a Party as a guest need no Account.
_Avoid_: Provider Connection, user profile

**Guest Token**:
A credential minted when a device without an Account redeems a Party Invite. It
is bound to that device's Device Key and to one Party, expires with the Party,
and is billed to the host's quota.
_Avoid_: Anonymous account, guest login

**Device Key**:
A per-installation key pair kept in the platform credential store and registered
to an Account. It proves which device a connection comes from.
_Avoid_: Device token, installation id

**Relay**:
The `ristle-server` component that forwards Party Commands, Party State and
File Share signaling between members. It checks identity and membership, and it
issues TURN credentials. It never applies queue logic.
_Avoid_: Party server, backend
