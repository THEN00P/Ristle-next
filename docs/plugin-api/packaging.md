# Plugin Packages

A Plugin Package is one `.wasm` file: a WebAssembly component built against
`ristle:plugin` (`plugin-api/wit/`). There is no archive. The manifest, the
icon and the Publisher signature travel in custom sections of the component.

Status: draft. Spike C (item 4, `docs/research/plugin-host-spike.md`)
signed and verified components with `wasmsign2` 0.2.8.

## Layout

Custom sections at the top level of the component, in this order:

| Section | Content | Limit |
|---|---|---|
| `signature` | Publisher signature in the WebAssembly signatures format (`wasmsign2`). Must be first | 4 KiB |
| `ristle-manifest` | The manifest, UTF-8 TOML | 64 KiB |
| `ristle-icon` | Square PNG, 256 × 256 to 1024 × 1024 | 64 KiB |
| `ristle-handover` | Optional. A handover signed by the previous Publisher key | 1 KiB |

The rest of the file is the component itself. A whole package is at most
20 MiB.

The icon is untrusted input shown before anything is accepted, so its
decoded size is bounded too: the host reads the dimensions from the PNG
header and refuses anything outside 256–1024 pixels square before decoding,
then stores a downscaled copy. SVG is not accepted, because a small file can
demand unbounded rendering work.

Custom sections have no effect on execution, so wasmtime loads the component
unchanged. The host reads the sections with a plain section parser before it
compiles anything.

## Signature

The signature follows the WebAssembly tool-conventions signature format that
`wasmsign2` implements:

- The `signature` custom section is the first section.
- Ed25519 keys; the module bytes after the signature section are hashed with
  SHA-256 and signed as one part (no `signature_delimiter`, no partial
  verification). The manifest, icon and handover sections are therefore
  covered.
- The section may carry several signatures. Ristle accepts a package when the
  installed Publisher key is among them, which is also how a key rotation
  without takeover works (see below).

**Components.** `wasmsign2` 0.2.8 accepts components as they are: it puts
its section first, hashes every section after it (so the manifest is
covered), rejects a tampered copy and a foreign key, adds about 120 bytes and
verifies in under a millisecond, and wasmtime loads the signed component
unchanged (spike C). Its signatures carry no public key, only an optional key
id, so the manifest names the Publisher key (`publisher-key`), and the host
verifies against that key and checks it against the installed one.

The Publisher is identified by its public key, and continuity of that key is
what Ristle checks. Users never see keys or fingerprints: the screens speak of
who made a Plugin, using the manifest's `publisher` name. Ristle does not
certify names, so the name is only shown, never trusted. The key fingerprint
appears only under "Technical details" in a Plugin's settings, for
developers.

## Manifest

```toml
id = "dev.example.soundcloud"         # reverse DNS, stable forever
name = "SoundCloud"
version = "1.4.0"                      # semver, must grow with every update
publisher = "Example Developer"
publisher-key = "ed25519:3fa1…"         # the key that signs this package
description = "Search, browse and play SoundCloud."
homepage = "https://github.com/example/ristle-soundcloud"   # HTTPS, opens in a browser
license = "MIT"
updates = "https://example.com/ristle/soundcloud.json"   # optional update feed, HTTPS

[provider]
id = "soundcloud"                      # several Plugins may serve one Provider
name = "SoundCloud"

[permissions]
hosts = ["api-v2.soundcloud.com", "api.soundcloud.com", "secure.soundcloud.com"]
media-hosts = ["*.sndcdn.com"]         # stream URLs the host fetches for the Plugin
image-hosts = ["*.sndcdn.com"]         # artwork and avatars the host fetches
sockets = []                           # "host:port", for the socket import
# Hosts the sign-in window may load. Omitted or empty: no web capture.
web-capture = ["soundcloud.com", "*.soundcloud.com", "accounts.google.com"]
# The subset whose traffic, cookies and pages may be watched and scripted:
# the Provider's own hosts, never an identity provider.
web-capture-observe = ["soundcloud.com", "*.soundcloud.com"]
storage-mib = 5
# servers = true                       # self-hosted: may ask the user for a server

[auth]
redirects = ["loopback:8990/login", "scheme", "remote"]
# app-schemes = ["soundcloud"]         # schemes an `opener: app` URL may use

[limits]                               # the host never exceeds these per call
tracks = 50
liked-states = 40
playlist-add = 100
playlist-remove = 100

[party]
# Fields a Party Host may relay to guests besides an item's reference, which
# is always relayed. Names are WIT field names of `track`, `album`, `profile`
# or `playlist`, checked by `ristle-plugin pack`; a name covers that field in
# every one of the four that has it, including guest search results. Omitted
# means every field; empty means references only (ADR 0012). Never
# credentials, stream sources, or anything outside those four records, such
# as a listing entry's `added-by`. `artwork` is relayed as URLs only for
# images on public HTTPS hosts that no `attach` rule covers, and whose URL
# has no query string, userinfo or fragment, which could carry a signature or
# token. Any other image, such as one from a Server Grant origin or a
# pre-signed URL, is fetched by the Party Host and relayed as the image
# itself, scaled down, so guests never learn a private address or receive a
# credential. An item's `url` passes the same test and is left out when it
# fails, since a link cannot be relayed as content.
share = ["title", "artists", "album", "artwork", "duration-ms"]

[playback]
# false: no crossfade or other overlap into or out of this Provider's Tracks
# (ADR 0012). Defaults to true.
overlap = true
```

What is *not* in the manifest:

- **Capabilities.** They are the interfaces the component exports
  (`accounts`, `likes`, `playlist-editing`...). The host reads them from the
  component's type at install time.
- **The API version.** It is in the import and export names
  (`ristle:plugin/catalog@0.1.0`). The host refuses a package whose version it
  does not implement and says which Ristle version is needed.
- **Callback configuration per connection** and anything about credentials.
  Those are runtime data (CONTEXT.md, Credential Record).

`auth.redirects` lists the OAuth redirects the Publisher registered with the
Provider. `loopback:<port>/<path>` is `http://127.0.0.1:<port>/<path>` on
desktop. `scheme` is Ristle's own custom scheme on mobile. `remote` is
Ristle's hosted HTTPS redirect page, which forwards to the app. The host picks
the first one the platform can serve and passes it as the `redirect-target`.

`ristle-plugin pack` validates the manifest against a schema, checks that the
component exports `catalog`, that `limits` are present for the exports that
need them, and that every host in the code's string table appears in
`permissions.hosts` (a lint, not a guarantee). It then writes the sections and
signs.

## Permissions and consent

The consent screen at install lists, in plain words:

- "Connects to" the `hosts`, `media-hosts` and `image-hosts`, and "Checks
  for updates at" the host of `updates`.
- "Shares details of Tracks, albums, artists and playlists with Party
  guests" unless `[party] share` is empty,
  naming the details when the list is short. Omitting `share` grants every
  field, and the screen says so.
- "Stores up to N MB on this device" when `storage-mib` is above the
  default of 5. The host refuses a manifest above 64.
- "Connects to servers you choose, including ones on your home network"
  when `servers = true`. Nothing is granted at install: each server is
  granted to one Provider Connection when the user picks and confirms it
  during sign-in.
- "Opens direct connections to" the `sockets`, separately and with a stronger
  wording, because they are not HTTPS.
- "Can see pages and traffic of these sites inside its own sign-in window"
  for the `web-capture-observe` hosts. Hosts only in `web-capture` are
  listed under "Technical details": the window may load them, nothing on
  them is watched.
- "Can open these apps to sign in" for `[auth] app-schemes`, naming the
  apps the system resolves the schemes to, or the schemes when it cannot.
- What it can do with an account, from its exports: "Signs in as you and
  can do anything your account can on the service" (`accounts`). That line
  is the real authority: whoever holds a sign-in can call the service's API
  with it, and a web-capture sign-in page can do the same, so the host cannot
  confine a signed-in Plugin to what its exports name. The export lines say
  which of those things Ristle will ask the Plugin to do: "Can like, save
  and follow for you" (`likes`), "Can repost for you" (`reposts`), "Can create, change and delete your playlists"
  (`playlist-editing`), "Can show Ristle in the service's own apps and take
  commands from them" (`queue-projection`). Exports that only read
  (`catalog`, `like-lookup`, `feed`, `comments`, `settings`, `byte-source`)
  add no line, because everything they read is already covered by the
  sign-in.

It does not repeat what the Provider's own authorization page shows (scopes),
following legacy ADR 0009.

The host enforces each grant where it is used: an undeclared HTTP host fails
with `error.internal` and is logged, a `socket` connect outside `sockets`
fails, a stream URL outside `media-hosts` does not play, an image outside
`image-hosts` (or not HTTPS) shows as a placeholder (both unless it is on
a Server Grant origin), and a web-capture
challenge whose start URL, observed hosts or script prefixes fall outside
`web-capture` is refused before any window opens. The sign-in window blocks
every request to hosts outside the list, from navigation to subresources,
`fetch`, beacons, frames and WebSockets.

## Installing

1. Read the sections. Reject a package without `signature` as first section,
   without a manifest, or over the size limits.
2. Read only `id` and `publisher-key` from the still untrusted manifest, to
   choose the key to verify with. Nothing else in it is used yet.
3. Verify the signature over the rest of the file with the manifest's
   `publisher-key`. If that `id` is installed, the result is then compared
   with the installed Publisher key (or the key a valid handover names) to
   tell an update from a takeover in step 6.
4. Parse and validate the whole manifest. Check `id` and `version`, and
   refuse a `homepage` that is not HTTPS; it is shown as a link that opens
   in a browser, never handed to the operating system.
5. Load the component with wasmtime and check its imports and exports
   against the `ristle:plugin` versions the host implements. A component
   whose declared minimum memory or table size exceeds the instance limits
   below is refused here.

Every instance runs under a wasmtime resource limiter that refuses growth
beyond 64 MiB of linear memory and 10 000 table elements; the Plugin then
gets an allocation failure or a trap, never more memory. Memories are
configured with small reservations, so a limit holds on devices with a
tight address space too (iOS). These limits are fixed by the host, not
chosen by the manifest.
6. Decide new install, update or takeover (below) and show the matching
   screen.
7. Store the package, then compile it once (Cranelift, or Pulley bytecode on
   iOS) and keep the compiled artifact, so later starts skip compilation.

## Updates

An update is a package with the same `id`, signed by the installed Publisher
key, with a higher `version`.

- **No new permissions:** installed without asking. Removed permissions and
  narrower limits never ask.
- **A new `web-capture-observe` host** is a new permission even when it
  was already in `web-capture`, since watching a site is more than loading
  it.
- **A new update-feed host** (`updates` moving to another host, or being
  added) is a new network destination and needs consent like a new host.
- **Loosened restrictions count as new permissions.** Sharing more fields
  with Party guests (`[party] share` gains fields, or is removed and falls
  back to every field), allowing overlap (`[playback] overlap` from `false`
  to `true`), new `[auth] app-schemes`, a larger `storage-mib` and `servers`
  turning on are shown as a difference and need consent like a new host.
  A Party difference names every item type that gains a field, such as
  "now shares Track details and playlist descriptions with Party guests".
- **New permissions:** the user sees only the difference ("now also connects
  to `api.example.com`", "now opens direct connections to ...") and must
  accept it. Until then the old version keeps running.
- **New exports** that add a consent line (for example `playlist-editing`
  or `queue-projection`) are permissions like any other: the difference is
  shown and must be accepted. New read-only exports do not ask.
- **Lower version:** refused, unless the user installs it by hand, with a
  warning. A downgrade still needs the same key.

Credential Records, storage and Provider Connections carry over unchanged,
with one exception: an update that drops `servers = true` ends every Server
Grant of the Plugin, and those connections need signing in again. Items they
returned stay bound to their connection (see `catalog` in `provider.wit`).

Installing any version (an update, a downgrade or a takeover) drops the
Plugin's cached HTTP responses and page, listing and Feed results before
the new code runs, since a new version may map them differently. What the
user saved, such as Ristle Playlist entries and the library, keeps its
Provider ids, so every version must keep accepting the ids earlier versions
returned. Active Stream Sources and live channels are retired too (see
`stream-source` in `types.wit` and `live` in `host.wit`).

### Key rotation

A Publisher who changes keys ships a package that

1. is signed by the old key, so it is an ordinary update, and
2. carries a `ristle-handover` section naming the new key, which the old key's
   signature covers like every other section.

The update installs like any other, but the host records the new key as
the Publisher key only after the user accepts the handover screen (see
Takeovers), worded for the same maker: "Example Developer now signs
SoundCloud differently. Accept only if you expect this from them." Until
then the old key stays the Publisher key and later packages signed by the new
key are takeovers. A rotation is never silent, because a stolen old key could
otherwise name its thief's key and outlive its own revocation. The handover
is what binds the successor: a second signature on its
own never changes the Publisher key. Because the `signature` section is not
part of the signed bytes, anyone can append a valid signature of their own
to a genuine update; that signature is ignored.

## Takeovers

A package with an installed `id` that is not signed by the installed key is a
takeover: someone other than the original maker is offering a replacement. It
is possible, but never silent, and the screen has to make sense to someone who
has never heard of signing.

The screen says, in this spirit:

> **This isn't an update from the original maker.**
> "SoundCloud" on this device was made by *Example Developer*. This file says
> it comes from *Other Person*, and Ristle cannot confirm that Example
> Developer agreed to hand it over.
> If you continue, Other Person's version replaces it and can use your
> SoundCloud sign-in. Only continue if you trust Other Person.
>
> [Keep the current version] [Replace and sign out] [Replace]

- The default button keeps what is installed.
- "Replace and sign out" removes every stored sign-in for the Plugin first,
  and with it everything the old Plugin kept that the host cannot vouch
  for: its whole `storage` namespace, its Plugin settings, its Server
  Grants and every cached response and result. The new maker starts with
  nothing of the user's accounts. Provider Connections stay as entries
  that need signing in, so Ristle Playlist references survive.
- The names come from the manifests and are shown as text, never as proof.
  Keys and fingerprints are not shown.

**With a handover** the wording changes, but the screen stays: "Example
Developer has handed SoundCloud over to Other Person." A handover is a
`ristle-handover` section that the *old* key signed over
`id ‖ new public key ‖ issued-at`, for example when a project changes
maintainers who cannot share a key. It does not remove the question, because
a stolen old key can also sign handovers. A handover from a revoked key is
never honoured (see Revocation).

After acceptance the new key is the Publisher key, and "Replaced by Other
Person on 12 Oct" appears in the Plugin's history in settings.

Installing a package whose `id` matches nothing installed, but which was
installed earlier and removed, is a new install: the host forgets removed
Publishers.

## Update feeds

A manifest may name an HTTPS update feed. Without one, the Plugin is updated
by installing a newer file by hand.

```json
{
  "id": "dev.example.soundcloud",
  "releases": [
    {
      "version": "1.5.0",
      "url": "https://example.com/ristle/soundcloud-1.5.0.wasm",
      "sha256": "9f2c…",
      "api": "0.1",
      "notes": "Fixes sign-in after SoundCloud's October change."
    }
  ],
  "revoked": []
}
```

- **When it is read.** When the user opens the Plugins page or taps "Check
  for updates", and once at app start at most every 24 hours. A feed
  response is at most 256 KiB and must arrive within 15 s; past either the
  host stops reading and treats the feed as unreachable. A check only
  sets a badge on the Plugins page. Nothing is downloaded or installed in the
  background.
- **Installing** is one tap ("Update all", or per Plugin). The host downloads
  the file, checks `sha256`, and then treats it exactly like a file the user
  picked: signature, Publisher key, version and permission diff. An update
  that needs new permissions shows the diff; one that would be a takeover
  shows the takeover screen. The feed adds no trust of its own, so a
  compromised feed can at worst offer a package the user then refuses.
- **The feed request and download URLs** must be HTTPS on the host of
  `updates`, and redirects are followed only within it. A feed that
  redirects elsewhere counts as unreachable, and a release whose `url`
  points elsewhere is ignored, so a feed cannot send the host to a destination the
  user never consented to. Publishers who serve files from a CDN put the
  feed there too.
- **`api`** is the `ristle:plugin` version the release needs. The host offers
  the newest release it can run and says when a newer one needs a newer
  Ristle.

## Revocation

A Publisher whose key leaked publishes a revocation in its feed:

```json
"revoked": [{ "key": "<public key>", "at": "2026-11-02T10:00:00Z", "signature": "<signed by that key>" }]
```

The revocation is signed by the revoked key itself. Anyone holding the key
can revoke it, which is harmless: revoking only ever removes trust.

When a check finds the installed Publisher key revoked, the host stops
accepting anything signed by it, and shows on the Plugin, in plain words:
"The maker of SoundCloud reports that their signing was compromised. Updates
are paused until you install a version they publish again." The installed
version keeps running, because it was installed before the leak was
reported.

Handovers signed by a revoked key are ignored, whatever their `issued-at`
says: the date is signed by the same key, so a thief could backdate it. The
next package from the Publisher's new key is therefore a plain takeover, with
the full wording. That is the price of a leaked key, and it falls on the
case where caution is most warranted.

## Not covered here

- A shared catalog of Plugins (a store or index). Feeds are per Plugin; a
  catalog is a separate product decision.
