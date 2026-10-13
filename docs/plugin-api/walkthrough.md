# Plugin API walkthrough: SoundCloud and Spotify on `ristle:plugin@0.1.0`

This document tests the WIT in `plugin-api/wit/` against the two real Plugins
in Ristle legacy (`packages/soundcloud-provider-plugin`,
`packages/spotify-provider-plugin`). It goes domain by domain and shows the
calls a Plugin author would write. Where a mapping came out awkward, the WIT
was changed; those changes are listed in [What the walkthrough changed](#what-the-walkthrough-changed).
The decisions the user still has to make are at the end.

Status: draft for review. Nothing here has been compiled except the WIT
itself (`wasm-tools component wit plugin-api/wit` and
`wit-bindgen rust --world full-provider`). The Rust below is a sketch against
the generated bindings, not tested code.

## How to read the code

The code is Rust against the types `wit-bindgen` generates. It assumes seven
helpers that the Ristle SDK will provide, each a few lines over the generated
types:

```rust
fn get(url: impl Into<String>) -> Request;      // GET, no headers, no body, cache: Http
impl Request {
    fn header(self, name: &str, value: impl Into<String>) -> Self;
    fn ttl(self, seconds: u32) -> Self;          // cache: Override { ttl, stale: ttl, key: None }
    fn fresh(self) -> Self;                      // cache: Bypass
}
fn json<T: DeserializeOwned>(r: Response) -> Result<T, Error>;
// 2xx → T; 404/410 → Unavailable; 5xx → Transient; other → Internal(status + body excerpt)
impl From<Item> for Entry { /* added: None, added_by: None */ }
fn ask(challenge: Challenge, state: &str) -> Step;  // Step::Ask(Ask { challenge, state })
```

Every export receives `conn: &Connection`. All HTTP goes through
`conn.fetch(&request)` or `conn.fetch_all(&requests)`, which attach the
Credential Record and apply rate limits and caching. Errors share one type, so
`?` works across imports and exports.

## The shape of a Plugin

A Plugin is one Rust crate in its own repository (ADR 0010). Its world says
which optional interfaces it implements:

```wit
// soundcloud/wit/world.wit
package ristle-plugins:soundcloud;
world plugin {
    include ristle:plugin/provider@0.1.0;    // catalog, plus every host import
    export ristle:plugin/accounts@0.1.0;
    export ristle:plugin/likes@0.1.0;
    export ristle:plugin/playlist-editing@0.1.0;
    export ristle:plugin/reposts@0.1.0;
    export ristle:plugin/feed@0.1.0;
    export ristle:plugin/comments@0.1.0;
    export ristle:plugin/settings@0.1.0;
}
```

```wit
// spotify/wit/world.wit
package ristle-plugins:spotify;
world plugin {
    include ristle:plugin/provider@0.1.0;
    export ristle:plugin/accounts@0.1.0;
    export ristle:plugin/likes@0.1.0;
    export ristle:plugin/like-lookup@0.1.0;
    export ristle:plugin/playlist-editing@0.1.0;
    export ristle:plugin/settings@0.1.0;
    export ristle:plugin/queue-projection@0.1.0;
}
```

SoundCloud does not export `like-lookup`: the host answers liked state from
its liked-tracks shelf (see [Likes](#likes)). Spotify has no feed, comments
or reposts, and SoundCloud no queue API, so neither exports those. Neither
exports `byte-source`.

The static facts (id, hosts, redirects, batch limits) live in the manifest,
described in `packaging.md`. The parts that matter here:

```toml
# SoundCloud
[permissions]
hosts = ["api-v2.soundcloud.com", "api.soundcloud.com", "secure.soundcloud.com"]
media-hosts = ["*.sndcdn.com", "*.media-streaming.soundcloud.cloud"]
image-hosts = ["*.sndcdn.com"]
# Every host the sign-in window may contact; all others are blocked. Google
# and Apple are there for "Continue with Google/Apple", DataDome for its bot
# check. Only SoundCloud hosts are observed. The exact list is to be recorded
# from real sign-in traffic, as the favorites requests were.
web-capture = ["soundcloud.com", "*.soundcloud.com", "*.sndcdn.com",
               "accounts.google.com", "*.gstatic.com", "appleid.apple.com",
               "*.captcha-delivery.com", "*.datadome.co"]
[auth]
redirects = ["loopback:8990/login", "scheme", "remote"]
[limits]
tracks = 50

# Spotify
[permissions]
hosts = ["api.spotify.com", "accounts.spotify.com", "spclient.wg.spotify.com",
         "apresolve.spotify.com", "clienttoken.spotify.com", "dealer.spotify.com"]
media-hosts = ["*.scdn.co", "*.spotifycdn.com", "audio-ak-spotify-com.akamaized.net"]
image-hosts = ["i.scdn.co", "*.spotifycdn.com"]
sockets = ["*.spotify.com:4070", "*.spotify.com:443"]
[auth]
redirects = ["loopback:8989/login", "scheme"]
[limits]
tracks = 50
liked-states = 40
playlist-add = 100
playlist-remove = 100
[party]
share = []          # only the Track reference reaches Party guests (ADR 0012)
[playback]
overlap = false     # no crossfade at Spotify boundaries (ADR 0012)
```

## What Plugins no longer do

Before the domains, the code that disappears. Roughly a third of each legacy
Plugin is plumbing the host now owns:

| Legacy Plugin code | Now |
|---|---|
| `SoundCloudState`/`SpotifyState`: connection, pending authorization, stream table | Host-held. Instances are disposable; the session `state` string, the Credential Record and `storage` carry everything |
| Five TTL caches in SoundCloud, search cache and saved-tracks page cache in Spotify | HTTP cache with `ttl` overrides, plus host-side result caching |
| `configure`, `ensure_fresh_tokens`, `validate_credentials`, `connected_status` | Host calls `refresh-credential` before `expires-at` |
| `record_rate_limit`, `search_retry_after_seconds` | Host turns 429 into `error.rate-limited` and backs off |
| `require_connection`, `validate_entity_request` | The host routes by connection and Provider |
| `classify_failure_message` on strings | Typed `error` cases |
| `PlayableSequence*` alongside every page type | One collection model: `list` serves both browsing and shuffle |
| Liked-id index with page budgets (SoundCloud) | Host derives liked state from the liked-tracks shelf |
| `read_audio`/`seek_audio`/PCM ring buffer (Spotify) | Host decodes; Spotify returns an encrypted progressive source |
| Pending-evaluation state machine for web capture | Declarative capture rules and page scripts |

## Sign-in

### SoundCloud: web capture

SoundCloud has two sign-in modes in legacy: OAuth with registered application
credentials, and capturing the web client's session when there are none.
The web session is what users actually get, so it is shown in full.

The legacy Plugin ran an interactive loop: every batch of observations came
to the Plugin, which tracked where the browser was and decided which script to
evaluate next, with retry spacing because adapters report navigation at
different moments. Under 0.1 the Plugin declares the whole thing once:

```rust
const DESKTOP_SIGN_IN_FORM: &str = r#"(function poll(){
  var f = document.querySelector('iframe[src*="secure.soundcloud.com/web-auth"]');
  if (!f) { return setTimeout(poll, 250); }
  location.href = f.getAttribute('src').replace('embedded_in_iframe=true', 'stand_alone=true');
})()"#;

fn begin_authorization(conn: &Connection, cx: SessionContext) -> Result<Step, Error> {
    if !cx.supported.contains(ChallengeKinds::WEB_CAPTURE) {
        return Err(Error::Unsupported("SoundCloud sign-in needs a web view".into()));
    }
    let api = |name: &str| Source::QueryParam(QueryParam {
        url_prefix: "https://api-v2.soundcloud.com/".into(), name: name.into() });
    let value = |name: &str, sources: Vec<Source>, required| Capture {
        name: name.into(), sources, required };
    Ok(Step::Ask(Ask {
        state: String::new(),
        challenge: Challenge::WebCapture(WebCapture {
            start_url: "https://soundcloud.com/signin".into(),
            // The mobile site is a different application with its own API.
            user_agent: UserAgent::Desktop,
            observe: vec!["soundcloud.com".into(), "*.soundcloud.com".into()],
            values: vec![
                value("oauth_token", vec![Source::Cookie("oauth_token".into())], true),
                value("client_id", vec![api("client_id")], true),
                value("app_version", vec![api("app_version")], true),
                value("app_locale", vec![api("app_locale")], true),
                value("datadome", vec![Source::RequestHeader("x-datadome-clientid".into()),
                                       Source::Cookie("datadome".into())], true),
                value("user_agent", vec![Source::RequestHeader("user-agent".into()),
                                         Source::Script], true),
            ],
            scripts: vec![
                // Most specific first: the callback shares the sign-in prefix.
                PageScript { url_prefix: "https://soundcloud.com/signin/callback".into(),
                             code: "location.href='https://soundcloud.com/discover'".into() },
                PageScript { url_prefix: "https://soundcloud.com/signin".into(),
                             code: DESKTOP_SIGN_IN_FORM.into() },
                PageScript { url_prefix: "https://soundcloud.com/".into(),
                             code: "ristle.capture('user_agent', navigator.userAgent)".into() },
            ],
        }),
    }))
}
```

The host opens the window, runs the matching script on each page load,
watches cookies and api-v2 requests on SoundCloud hosts, and closes the
window when all six values have been seen. Because the completion predicate
is the `required` flags, it cannot drift away from what the Plugin later
needs, which was a real legacy bug ("sign-in appears to succeed and the next
launch silently disconnects"). The host refuses the challenge up front on a
platform that cannot set a desktop user agent, so the "landed on the mobile
client" failure becomes impossible rather than detected.

The answer becomes a Credential Record whose `attach` rule reproduces
legacy's `web_request_builder` once, so no request in the Plugin adds auth by
hand:

```rust
fn continue_authorization(conn: &Connection, _cx: SessionContext, _state: String,
                          answer: Answer) -> Result<Step, Error> {
    let Answer::WebCapture(values) = answer else { return Err(unexpected()) };
    let v = |k: &str| values.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let credential = Credential {
        scheme: "web-session/1".into(),
        access_token: Some(v("oauth_token")),
        refresh_token: None, expires_at: None, scopes: vec![],
        username: None, password: None, custom: None,
        attach: vec![Attach {
            hosts: vec!["api-v2.soundcloud.com".into()],
            headers: vec![
                ("Authorization".into(), format!("OAuth {}", v("oauth_token"))),
                ("User-Agent".into(), v("user_agent")),
                ("x-datadome-clientid".into(), v("datadome")),
                // DataDome rejects favorites writes without it (measured 2026-07-28).
                ("Accept-Language".into(), v("app_locale")),
                ("Referer".into(), "https://soundcloud.com/".into()),
            ],
            query: vec![("client_id".into(), v("client_id")),
                        ("app_version".into(), v("app_version")),
                        ("app_locale".into(), v("app_locale"))],
        }],
    };
    conn.set_credential(&credential)?;                // so the next fetch is signed
    let me: V2Me = json(conn.fetch(&get("https://api-v2.soundcloud.com/me"))?)?;
    Ok(Step::Done(Authorized { credential, account: me.into() }))
}
```

`query` is appended only when the URL lacks the parameter, which replaces
legacy's `web_request_builder_with_existing_context` that checked `next_href`
cursors for an existing `client_id`.

**DataDome rotation.** SoundCloud refreshes its DataDome value in an
`x-set-cookie` response header and expects it on the next request. The Plugin
keeps one wrapper for that. It reads the record before sending, so the
compare-and-set in `set-credential` fails if another response rotated the
value in the meantime, and an older rotation never overwrites a newer one:

```rust
fn sc(conn: &Connection, req: Request) -> Result<Response, Error> {
    let sent_with = conn.credential();               // read before sending: the CAS base
    let response = conn.fetch(&req)?;
    if response.cached { return Ok(response); }    // a cached header is stale
    if let Some(fresh) = datadome_from_x_set_cookie(&response.headers) {
        let mut c = sent_with.ok_or_else(unauthorized)?;
        set_attached_header(&mut c, "x-datadome-clientid", fresh);
        match conn.set_credential(&c) {
            Err(Error::Conflict(_)) => {}           // another call stored a newer record; keep it
            r => r?,
        }
    }
    Ok(response)
}
```

**Refresh and re-authorization.** A web session does not expire and cannot be
refreshed:

```rust
fn refresh_credential(_: &Connection) -> Result<Credential, Error> {
    Err(Error::Unauthorized("Sign in to SoundCloud again".into()))
}
```

When api-v2 answers 401, `conn.fetch` fails with `unauthorized`, the host
calls `refresh-credential`, gets `unauthorized` again, marks the connection as
needing sign-in, and later calls `begin-authorization` with
`reason: reauthorize`. The connection, its library cache and its Ristle
Playlist references survive. If the new account id differs, the host asks the
user before replacing anything, and on acceptance drops the connection's cached
responses and results. Choosing a different server for a self-hosted Provider
does the same.

**OAuth with registered application credentials** is the same as Spotify's
first step below and is not repeated.

### Spotify: two chained OAuth redirects

Legacy Spotify signs in twice in one session: once with the playback client
(the one librespot uses, for access points and spclient), then with the
catalog client (Web API). Both use the same registered redirect. In 0.1 that
is two `ask` steps:

```rust
fn authorize_url(client_id: &str, r: &RedirectTarget) -> String {
    format!("https://accounts.spotify.com/authorize?response_type=code&client_id={client_id}\
             &redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256&scope={}",
            enc(&r.uri), enc(&r.state), enc(&r.code_challenge), enc(&SCOPES.join(" ")))
}

fn redirect(url: String, state: &str) -> Step {
    Step::Ask(Ask { state: state.into(), challenge: Challenge::Redirect(RedirectChallenge {
        url, opener: Opener::Browser, fallback_url: None, keep: vec![] }) })
}

fn begin_authorization(_: &Connection, cx: SessionContext) -> Result<Step, Error> {
    let r = cx.redirect.ok_or_else(|| Error::Unsupported("no OAuth redirect here".into()))?;
    Ok(redirect(authorize_url(PLAYBACK_CLIENT, &r), "playback"))
}

fn continue_authorization(conn: &Connection, cx: SessionContext, state: String,
                          answer: Answer) -> Result<Step, Error> {
    let Answer::Redirect(cb) = answer else { return Err(unexpected()) };
    let code = param(&cb.params, "code").ok_or_else(|| denied(&cb.params))?;
    if state == "playback" {
        let playback = exchange(conn, PLAYBACK_CLIENT, code, &cb)?;
        // The second step gets a fresh redirect target from the host.
        let r = cx.redirect.ok_or_else(unexpected)?;
        return Ok(redirect(authorize_url(CATALOG_CLIENT, &r),
                           &serde_json::to_string(&playback).unwrap()));
    }
    let playback: Token = serde_json::from_str(&state).map_err(|_| unexpected())?;
    let catalog = exchange(conn, CATALOG_CLIENT, code, &cb)?;
    let credential = credential(&catalog, &playback);
    conn.set_credential(&credential)?;         // a candidate until the host accepts `done`
    let me: SpotifyUser = json(conn.fetch(&get("https://api.spotify.com/v1/me"))?)?;
    Ok(Step::Done(Authorized { credential, account: me.into() }))
}

fn exchange(conn: &Connection, client: &str, code: &str, cb: &Callback) -> Result<Token, Error> {
    let body = form(&[("grant_type", "authorization_code"), ("code", code),
                      ("redirect_uri", &cb.redirect_uri), ("client_id", client),
                      ("code_verifier", &cb.code_verifier)]);
    json(conn.fetch(&post_form("https://accounts.spotify.com/api/token", body))?)
}

fn credential(catalog: &Token, playback: &Token) -> Credential {
    Credential {
        scheme: "two-clients/1".into(),
        access_token: Some(catalog.access_token.clone()),
        refresh_token: Some(catalog.refresh_token.clone()),
        // The host refreshes before the earlier of the two expires.
        expires_at: Some(catalog.expires_at.min(playback.expires_at)),
        scopes: catalog.scopes.clone(),
        username: None, password: None,
        custom: Some(serde_json::to_string(playback).unwrap()),
        attach: vec![Attach { hosts: vec!["api.spotify.com".into()],
            headers: vec![("Authorization".into(), format!("Bearer {}", catalog.access_token))],
            query: vec![] }],
    }
}
```

The host owns `state`, the PKCE verifier and the loopback listener, so the
Plugin never sees anything but the code (legacy ADR 0009). The second token
lives in `custom` and the Plugin attaches it by hand on spclient requests and
the access point (see [Streams](#streams)).

`refresh-credential` refreshes both tokens and returns a new record. Legacy's
"missing `app-remote-control` scope" check becomes:

```rust
if !playback.scopes.iter().any(|s| s == "app-remote-control") {
    return Err(Error::Unauthorized("Reconnect Spotify to allow playback".into()));
}
```

Mobile: the manifest lists `scheme`, so on Android and iOS the host passes a
`ristle://` redirect target instead of the loopback one. The Plugin code does
not change. To sign in through the installed Spotify app (deep link), the
Plugin would return `opener: App` with the app's authorize URL and the web URL
as `fallback_url`; the callback still comes back through the redirect target.
The app URL is either an HTTPS link on `accounts.spotify.com` that the system
hands to the app, or a scheme the manifest lists in `[auth] app-schemes`;
spike E records which one the Spotify app answers.

## Search

Both Plugins answer one call per connection. The host asks for every group at
once and gets each group back with its own `next`. "Show more" on one group
passes that group's `next` as `cursor`, with only that kind, so it continues
the same search instead of starting over.

SoundCloud has one endpoint per group, so it fans out with `fetch_all`, like
legacy's `send_batch`:

```rust
fn search(conn: &Connection, req: SearchRequest) -> Result<SearchResults, Error> {
    let paths = [(SearchKind::Tracks, Kinds::TRACKS, "tracks"),
                 (SearchKind::Artists, Kinds::ARTISTS, "users"),
                 (SearchKind::Playlists, Kinds::PLAYLISTS, "playlists")];
    let asked: Vec<_> = paths.iter().filter(|(_, k, _)| req.kinds.contains(*k)).collect();
    let requests = asked.iter().map(|(_, _, path)| match &req.cursor {
        Some(next) => checked_v2_url(next).map(get),                 // one kind, next page
        None => Ok(get(format!("{V2}/search/{path}?q={}&limit={}&linked_partitioning=1",
                                enc(&req.query), req.limit)).ttl(60)),
    }).collect::<Result<Vec<_>, _>>()?;
    let groups = conn.fetch_all(&requests)?.into_iter().zip(&asked).map(|(response, (kind, _, _))| {
        let page: Collection<Any> = json(response?)?;
        Ok(SearchGroup { kind: *kind, items: page.items(), next: page.next_href })
    }).collect::<Result<Vec<_>, Error>>()?;
    Ok(SearchResults { groups })
}
```

Spotify has one endpoint with `type=track,album,artist,playlist`, which
returns a separate pager per type; each pager's `next` URL becomes that
group's `next`. The one-hour search cache in legacy becomes
`.ttl(3600)` on the request. Spotify playlist owners map to `Item::User`.

Legacy's `SearchScope::MyLibrary` is gone. Both Plugins answered it by
loading their whole library into memory and filtering locally, which the host
can do better over the collections it already enumerates (see the decision
list).

## Tracks and hydration

`tracks` takes up to `limits.tracks` ids and returns them in order:

```rust
// SoundCloud
fn tracks(conn: &Connection, ids: Vec<String>) -> Result<Vec<Result<Track, Error>>, Error> {
    let found: Vec<V2Track> = json(sc(conn, get(format!(
        "https://api-v2.soundcloud.com/tracks?ids={}&limit=50", ids.join(","))))?)?;
    Ok(ids.iter().map(|id| found.iter().find(|t| t.id.to_string() == *id).map(track)
        .ok_or_else(|| Error::Unavailable("Removed from SoundCloud".into()))).collect())
}
```

Legacy Spotify used `/v1/tracks?ids=`, which Development Mode apps lost in
2026; the port reads the same batch from spclient (see
[Spotify's Web API in Development Mode](#spotifys-web-api-in-development-mode)).
Neither Plugin chunks: the host never sends more than 50.

The mapping function is where most of a Plugin's lines go, and it is no
longer than legacy's `track_value`:

```rust
fn track(t: &V2Track) -> Track {
    Track {
        id: t.id.to_string(),
        title: t.title.clone(),
        artists: vec![Named { id: Some(t.user.id.to_string()), name: t.user.username.clone() }],
        album: None,
        duration_ms: t.duration,
        artwork: artwork(t.artwork_url.as_deref()),          // -t500x500 upscaling as before
        traits: TrackTraits::empty(),
        released: t.release_date.as_deref().and_then(media_date),
        uploaded: t.created_at.as_deref().and_then(media_date),
        unavailable: unplayable_reason(t).map(str::to_owned), // AAC-HLS only, as in legacy
        isrc: t.publisher_metadata.as_ref().and_then(|m| m.isrc.clone()),
        url: Some(t.permalink_url.clone()),
        attributes: genre(t),
    }
}
```

Availability keeps legacy's rule: "playable" means exactly what `resolve` can
open (unencrypted AAC-HLS), so label Tracks that offer only DRM HLS are listed
as unavailable rather than failing at play time.

## Pages are data

An earlier draft returned every page as a list of generic sections (rows,
shelves, grids) that the Plugin arranged. That is a layout language in
disguise: every Plugin would arrange an artist page differently, and Ristle
could not place "Popular" or "Releases" consistently across Providers.

0.1 returns typed data instead. `open` answers with one of `album-page`,
`playlist-page`, `profile-page` (for artists and users) or `view-page`, and
the host has one layout per kind. A Plugin fills in the parts its Provider
has and leaves the rest `none`; the host shows what is present, always in the
same place. Generic sections remain only inside Provider Views, the escape
hatch for surfaces that exist on one Provider (charts, a curated hub). Their
`layout` is a variant so that new kinds can be added in later versions.

## Albums

**Spotify** albums are albums:

```rust
fn open_album(conn: &Connection, id: &str) -> Result<Page, Error> {
    let a: AlbumDetail = json(conn.fetch(&get(format!("https://api.spotify.com/v1/albums/{id}")))?)?;
    let album = album(&a);
    Ok(Page::Album(AlbumPage {
        tracks: Part {
            entries: a.tracks.items.iter().map(|t| Item::Track(simplified_track(t, &album)).into()).collect(),
            more: a.tracks.next.is_some().then(|| More {
                collection: Collection { id: format!("album:{id}"), addressing: Addressing::Offset,
                    total: Some(Count::Exact(a.tracks.total)), revision: None, endless: false },
                next: None }),
        },
        album,
    }))
}
```

Albums longer than one page continue through `list("album:{id}", offset)`.
The `album_type` maps to `release-type` as in legacy (`album`, `single`,
`compilation`; a single with more than three Tracks is an `ep`).

**SoundCloud** has no album entity: albums, EPs and singles are playlists
("sets") with a `set_type`. The Plugin returns them as `Item::Album` where the
set type says so, with the `set_type` mapped onto `release-type`, and as
`Item::Playlist` otherwise. `open(Target::Album(id))` uses the same
`/playlists/{id}` code as a playlist (see below) and returns `Page::Album`.

## Artists and users

Spotify has two kinds of account, and the API keeps them apart:

| | Spotify artist | Spotify user |
|---|---|---|
| Publishes | Tracks, albums, singles, compilations | Nothing |
| Owns | Nothing | Playlists |
| Page | Popular Tracks, releases, "appears on" | Public playlists |
| Follow | `PUT /me/following?type=artist` | `PUT /me/following?type=user` |
| Appears as | `track.artists`, `album.artists` | `playlist.owner`, a playlist's "added by" |

A musician who also curates playlists has both, with different ids, and
Spotify does not link them. Ristle shows them as two profiles too.

This is why links to accounts are a `person` variant (`artist` or `user`)
rather than a bare name and id: a playlist owner opens a user page, a Track's
artist opens an artist page, and a follow goes to the right endpoint.
`item-ref` and `target` have the same two cases.

SoundCloud has one kind of account that can both upload and own playlists,
so the SoundCloud Plugin reports every account as an artist. Both kinds of
page use the same `profile-page` record, so nothing is lost: a SoundCloud
profile simply fills more parts.

**SoundCloud** profile:

```rust
fn open_artist(conn: &Connection, id: &str) -> Result<Page, Error> {
    let [user, tracks, albums, playlists] = fetch4(conn, [
        get(format!("{V2}/users/{id}")).ttl(300),
        get(format!("{V2}/users/{id}/tracks?limit=20&linked_partitioning=1")).ttl(300),
        get(format!("{V2}/users/{id}/albums?limit=12&linked_partitioning=1")).ttl(300),
        get(format!("{V2}/users/{id}/playlists_without_albums?limit=12&linked_partitioning=1")).ttl(300),
    ])?;
    let user: V2User = json(user)?;
    Ok(Page::Artist(ProfilePage {
        profile: profile(&user),
        top_tracks: None,
        tracks: Some(part(json(tracks)?, format!("tracks:{id}"), |t| Item::Track(track(t)))),
        releases: Some(part(json(albums)?, format!("releases:{id}"), |p| Item::Album(album(p)))),
        playlists: Some(part(json(playlists)?, format!("playlists:{id}"), |p| Item::Playlist(playlist(p)))),
        // Opened lazily: only the collection is named here.
        likes: Some(Part { entries: vec![], more: Some(cursor_more(format!("likes:{id}"))) }),
        reposts: Some(Part { entries: vec![], more: Some(cursor_more(format!("reposts:{id}"))) }),
        appears_on: None,
        related: None,
        discography: Some(cursor_collection(format!("discography:{id}"))),
        extras: vec![],
    }))
}
```

`part` is the Plugin's own five-line helper that maps a SoundCloud
`linked_partitioning` page onto a `Part` with `more` from `next_href`.

`discography` is what Play and Shuffle use, which replaces legacy's
`top_tracks_sequence` and page-level `playable_sequence`. Its listing is
legacy's `artist-discography` sequence: release Tracks first, then the uploads
that were never part of a set. The phase lives in the opaque `next` token,
exactly as legacy's `DiscographySequenceCursor` did, and with `detail: Ids`
it returns `Item::TrackId` entries so that the host can enumerate a large
discography without fetching full Tracks.

Legacy's `artist-releases`, `profile-playlists`, `profile-likes` and
`profile-reposts` Provider Views are gone: they are now the `releases`,
`playlists`, `likes` and `reposts` parts, and the host shows them the same way
for SoundCloud, Audius or any other Provider that has them.

**Spotify** artist: `top_tracks` from spclient's artist metadata, which
lists the popular Tracks (the Web API's `/artists/{id}/top-tracks` is gone for
Development Mode apps; if spclient fails, the part is `none` and the page still
loads), `releases` from `/artists/{id}/albums?include_groups=album,single,compilation`
(offset-addressed, the host groups by `release-type`), `appears_on` from
`include_groups=appears_on`, and `discography` as legacy's discography
sequence. `related` stays `none`: Spotify closed related artists to new
applications in 2024.

**Spotify** user: `open(Target::User(id))` returns `Page::User` with
`playlists` and every other part `none`. The Web API's `/users/{id}` and
`/users/{id}/playlists` are gone for Development Mode apps, so the profile
and its public playlists come from spclient (the user's profile and
rootlist), which spike E checks. If spclient cannot serve them, the Plugin
returns `error.unavailable`. A stateless Plugin cannot know the name, but the
host can: it opened the page from a `person` it already shows (a playlist's
owner, a feed post), so it shows that name as the header with no parts.

## Playlists

### Reading

**SoundCloud** returns a playlist with its Track list in one response, the
first few Tracks complete and the rest as `{ "id": ... }` stubs. That maps
directly onto `Item::TrackId`:

```rust
fn list_playlist(conn: &Connection, id: &str) -> Result<Listing, Error> {
    let p: V2Playlist = json(sc(conn, get(format!("{V2}/playlists/{id}")).ttl(300))?)?;
    Ok(Listing {
        entries: p.tracks.iter().map(|t| match t {
            V2PlaylistTrack::Full(t) => Item::Track(track(t)).into(),
            V2PlaylistTrack::Stub { id } => Item::TrackId(id.to_string()).into(),
        }).collect(),
        next: None,
        total: Some(Count::Exact(p.track_count)),
        revision: p.last_modified.clone(),
    })
}
```

The host hydrates the stubs with `tracks` in batches of 50 when they scroll
into view, or not at all when the user only shuffles. Duplicate Tracks stay
distinct because the host numbers occurrences by position in the
enumeration; the Plugin no longer invents entry ids such as
`playlist:42@17`.

**Spotify** playlists are offset-addressed and carry a `snapshot_id`, which
is an exact revision. Since February 2026 the Web API returns items only for
playlists the user owns or collaborates on (others answer 403), at most 50
per page, with each entry's Track in `item`. The sketch below covers those;
for any other playlist the Plugin reads `playlist/v2` from spclient, with the
same addressing and revision. The items response has no `snapshot_id`, so the
Plugin reads the snapshot before and after the page. If they differ, the
playlist changed during the read, and the page could pair old positions with
the new snapshot; the Plugin returns `error.conflict` and the host reads the
page again. All three requests bypass the HTTP cache (`fresh`), because cached
copies could agree with each other while describing different revisions; the
host still caches the resulting listing:

```rust
fn list_playlist(conn: &Connection, id: &str, req: ListRequest) -> Result<Listing, Error> {
    let offset = match req.cursor { Cursor::Offset(o) => o, _ => 0 };
    let fields = match req.detail {           // cheap enumeration when the host only needs ids
        Detail::Ids => "&fields=total,items(item(id))",
        Detail::Items => "",
    };
    let snapshot_url = format!("https://api.spotify.com/v1/playlists/{id}?fields=snapshot_id");
    let before: Snapshot = json(conn.fetch(&get(&snapshot_url).fresh())?)?;
    let items = conn.fetch(&get(format!(
        "https://api.spotify.com/v1/playlists/{id}/items?offset={offset}&limit=50{fields}")).fresh())?;
    if items.status == 403 {                     // someone else's playlist
        return spclient_playlist(conn, id, req);
    }
    let page: PlaylistItems = json(items)?;
    let snapshot: Snapshot = json(conn.fetch(&get(&snapshot_url).fresh())?)?;
    if snapshot.snapshot_id != before.snapshot_id {
        return Err(Error::Conflict("Playlist changed while it was read".into()));
    }
    Ok(Listing {
        entries: page.items.into_iter().map(|i| match i.item {
            Some(t) if req.detail == Detail::Ids => Item::TrackId(t.id).into(),
            Some(t) => Entry { item: Item::Track(track(&t)), added: instant(&i.added_at),
                               added_by: i.added_by.map(user) },
            None => Item::Unavailable(Some("No longer on Spotify".into())).into(),
        }).collect(),
        next: None,
        total: Some(Count::Exact(page.total)),
        revision: Some(snapshot.snapshot_id),
    })
}
```

`Item::Unavailable` keeps the position (legacy's
`spotify-unavailable-playlist-ordinal:N` handle), so later positions stay
right and the host can show a Disabled Track.

### Folders

Spotify's folder layout comes from the rootlist on spclient, not from the Web
API. The `playlists` shelf lists folders first (`Item::Folder` with `parent`),
then playlists with `folder` set. Legacy's restart-the-listing logic for a
layout that loaded late is unnecessary: the host re-lists the shelf when its
revision changes.

### Writing

Spotify maps onto `/playlists/{id}/items` (renamed from `/tracks` in
February 2026). Two gaps in that endpoint need care:

- **Adds take no snapshot.** A position sent with the add is applied to
  whatever the playlist is at that moment, so a change in between would
  misplace it. A positional add is therefore two steps. The Plugin appends at
  the end, which no concurrent change can misplace. It then reads the
  snapshot and total fresh, and moves the new range to `at` with a reorder
  sent with that snapshot, which Spotify applies against the snapshot it
  names (spike E confirms this). The user's `revision` is still checked
  first, and the total after the append must be the total before plus the
  added Tracks. If either read shows another change or the reorder fails,
  the Tracks stay at the end and the Plugin returns `error.conflict`, so the
  host reads the playlist again and shows where they are. What remains is a
  change in the moment between the first read and the append that leaves
  the total unchanged, such as another client's reorder; the Tracks then
  land at `at` of the reordered list. Spotify offers no conditional add, so
  this window is accepted, as for SoundCloud, rather than giving up
  positional adds.
- **Removals take only URIs.** Each entry is `{ "uri": ... }`; there is no
  position, so a Track that appears more than once cannot be removed one
  occurrence at a time. When the playlist holds a removed Track only once
  (the Plugin checks with the listing it has read), the Web API removal is
  exact. Otherwise the Plugin removes that occurrence through spclient's
  position-addressed playlist changes (spike E), and if that path is
  unavailable it returns `error.unsupported` rather than removing the wrong
  copies.

```rust
fn edit(conn: &Connection, id: String, revision: Option<String>, change: Change) -> Result<Outcome, Error> {
    let url = format!("https://api.spotify.com/v1/playlists/{id}/items");
    let body = match change {
        Change::Add(i) if i.at.is_some() => return add_at(conn, &id, revision, i),
        Change::Add(i) => json!({ "uris": uris(&i.tracks) }),
        Change::Remove(o) => {
            if has_duplicates(conn, &id, &o)? {
                return spclient_remove_occurrences(conn, &id, revision, &o);
            }
            json!({ "snapshot_id": revision,
                    "items": o.iter().map(|o| json!({ "uri": uri(&o.track) })).collect::<Vec<_>>() })
        }
        Change::Move(m) => json!({ "snapshot_id": revision, "range_start": m.from,
                                   "range_length": m.count, "insert_before": m.to }),
        Change::Details(d) => return details(conn, &id, d),
    };
    let method = match change { Change::Add(_) => Method::Post, Change::Remove(_) => Method::Delete, _ => Method::Put };
    let r: Snapshot = json(conn.fetch(&with_json(method, url, body))?)?;
    Ok(Outcome { revision: Some(r.snapshot_id), rejected: vec![] })
}

fn add_at(conn: &Connection, id: &str, revision: Option<String>, i: Insert) -> Result<Outcome, Error> {
    let moved = || Error::Conflict("Playlist changed; the Tracks were added at the end".into());
    let before = current(conn, id)?;                  // `.fresh()` read
    if revision.is_some_and(|r| r != before.snapshot_id) {
        return Err(Error::Conflict("Playlist changed since it was shown".into()));
    }
    let url = format!("https://api.spotify.com/v1/playlists/{id}/items");
    let appended: Snapshot = json(conn.fetch(&with_json(Method::Post, &url, json!({ "uris": uris(&i.tracks) })))?)?;
    let now = current(conn, id)?;                     // snapshot_id and items.total, `.fresh()`
    let n = i.tracks.len() as u32;
    if now.snapshot_id != appended.snapshot_id || now.total != before.total + n {
        return Err(moved());                          // someone else changed it around the append
    }
    let r: Snapshot = json(conn.fetch(&with_json(Method::Put, &url, json!({
        "snapshot_id": now.snapshot_id, "range_start": now.total - n,
        "range_length": n, "insert_before": i.at })))?).map_err(|_| moved())?;
    Ok(Outcome { revision: Some(r.snapshot_id), rejected: vec![] })
}
```

The host splits large changes, so `edit` always fits in one request:

- A 250-Track Playlist Add becomes three chunks of `limits.playlist-add`
  (100). With an explicit `at`, the second chunk goes to `at + 100` and the
  third to `at + 200` (less any Tracks the earlier chunks rejected), so the
  order is kept.
- Removing 250 occurrences becomes three chunks of `limits.playlist-remove`
  (100, Spotify's maximum), highest positions first, so the positions in the
  later chunks are still right after the earlier ones.
- Each chunk carries the `revision` the previous one returned. If the third
  chunk fails, the host reports "Added 200 of 250"; the first two stay.

`rejected` is for Providers that refuse individual items inside a request
that otherwise succeeded.

SoundCloud's API replaces a playlist's whole Track list in one `PUT`, so its
`edit` reads the current list, applies the change, and writes it back. If
`last_modified` no longer matches `revision`, it returns `error.conflict`.
That check runs before the write, so a change another client makes in the
round trip between the read and the `PUT` would be overwritten. The Plugin
sends `If-Unmodified-Since` with the `last_modified` it read; if the recorded
observations show api-v2 honouring it (412 maps to `error.conflict`), the
write is conditional. If not, the window of one round trip stays. That is
accepted rather than leaving SoundCloud playlist editing out, which the user
decided against.
Legacy declared `playlist_write: false` for SoundCloud and never built
writes. The port supports them, as Spotify's does. The web-session request
shapes (`PUT /playlists/{id}`, `POST /playlists` and `DELETE /playlists/{id}`
on api-v2, and whether the `PUT` honours `If-Unmodified-Since`) still have to
be observed in the browser and recorded, as the
favorites requests were in `soundcloud-favorites-web-observations.md`.

`create` and `delete` are single requests on both services. Unfollowing
someone else's playlist is `set-liked(ItemRef::Playlist(id), false)`.

## Library

`my-library` lists the standard shelves. The host shows them under My Library,
searches them for My Library Search, and enumerates them for liked state.

```rust
// SoundCloud
fn my_library(conn: &Connection) -> Result<Library, Error> {
    let shelf = |kind, title: &str, id: &str| Shelf { kind, title: title.into(),
        collection: cursor_collection(id.into()) };
    Ok(Library {
        shelves: vec![
            shelf(LibraryKind::LikedTracks, "Likes", "likes"),
            shelf(LibraryKind::Playlists, "Playlists", "playlists"),
            shelf(LibraryKind::SavedAlbums, "Albums", "albums"),
            shelf(LibraryKind::FollowedArtists, "Following", "following"),
            shelf(LibraryKind::Uploads, "Your tracks", "uploads"),
        ],
        views: vec![],         // the feed is its own export, not a view
    })
}
```

SoundCloud's `likes` collection has two ways to list it, chosen by `detail`:

```rust
fn list_likes(conn: &Connection, req: ListRequest) -> Result<Listing, Error> {
    let me = conn.account().ok_or_else(unauthorized)?.id;
    let first = match req.detail {
        // Full items, with the date each Track was liked.
        Detail::Items => format!("{V2}/users/{me}/track_likes?limit={}&linked_partitioning=1", req.limit.min(200)),
        // 200 ids per request: what the web player itself uses for liked state.
        Detail::Ids => format!("{V2}/me/track_likes/ids?limit=200"),
    };
    let url = match req.cursor { Cursor::Next(n) => checked_v2_url(&n)?, _ => first };
    let page: Collection<Like> = json(sc(conn, get(url))?)?;
    Ok(Listing {
        entries: page.items.iter().map(|l| match l {
            Like::Id(id) => Item::TrackId(id.to_string()).into(),
            Like::Full { track: t, created_at } => Entry { item: Item::Track(track(t)),
                added: instant(created_at), added_by: None },
        }).collect(),
        next: page.next_href,       // a repeated cursor is the host's problem now
        total: None, revision: None,
    })
}
```

### Spotify Liked Songs: positional handles

This is the case the collection model was shaped around. `/v1/me/tracks`
pages by `offset` only and has no id-only projection. Legacy enumerated it by
emitting `total` synthetic handles (`spotify-liked-track-ordinal:N`) and
resolving them later through `?offset=N`. Any like or unlike shifts every
ordinal, so an unhydrated handle silently landed on a neighbor.

In 0.1 the Plugin declares the collection as offset-addressed and does
nothing special:

```rust
Shelf { kind: LibraryKind::LikedTracks, title: "Liked Songs".into(),
        collection: Collection { id: "liked".into(), addressing: Addressing::Offset,
                                 total: None, revision: None, endless: false } }

fn list_liked(conn: &Connection, req: ListRequest) -> Result<Listing, Error> {
    let offset = match req.cursor { Cursor::Offset(o) => o, _ => 0 };
    let page: Paging<SavedTrack> = json(conn.fetch(&get(format!(
        "https://api.spotify.com/v1/me/tracks?offset={offset}&limit={}", req.limit.min(50))))?)?;
    Ok(Listing {
        entries: page.items.iter().map(|s| Entry { item: Item::Track(track(&s.track)),
            added: instant(&s.added_at), added_by: None }).collect(),
        next: None,
        total: Some(Count::Exact(page.total)),
        // Spotify has no snapshot for Liked Songs. The total is a weak signal.
        revision: Some(page.total.to_string()),
    })
}
```

The host does the rest, which is where the safety belongs. Its rule is that
a change to the collection never reshuffles: the order the user can see stays
as it is, and changes are applied to what has not been decided yet.

1. **Shuffle without fetching.** To shuffle 5,000 Liked Songs, the host reads
   offset 0 once, learns the total, and shuffles the positions 0..5000. The
   shuffled order is a list of slots. A slot is either *resolved* (the host
   has read it and knows its Track id) or still a *position*.
2. **Resolve before showing or playing.** Before a slot reaches Up Next or
   plays, the host reads the page around it (offset 3,100 covers position
   3,117 and the neighbors it will need soon) and keeps the Track id. Up Next
   only ever shows resolved slots, so what the user sees is a real Track and
   stays that Track.
3. **A like or unlike made in Ristle is applied, not voided.** Spotify adds a
   new like at position 0, so every unresolved position moves down by one and
   the new Track is inserted at a random point among the slots still to come.
   An unlike removes that Track from the slots still to come; if its position
   is known, the positions behind it move up by one. Nothing already resolved
   moves, and nothing reshuffles.
4. **A change made elsewhere is found and re-based lazily.** The host notices
   it when a page's `revision` (the total, here) differs or a re-read page
   disagrees with Track ids it already knows. It does not throw the order
   away. When it next needs an unresolved position, it reads that page under
   the new revision and uses the Track ids it already knows on that page to
   work out how far positions moved. Tracks already played in this pass are
   skipped if they turn up again, and new likes discovered at the top join
   the remaining order at random points, as in step 3.

Legacy's saved-tracks page cache in the Plugin is gone; the host keeps fetched
pages for the life of the shuffle.

What 0.1 cannot fix: an unlike and a like made elsewhere between two reads
keep the total equal and shift positions. Step 4 detects that only on pages
the host has seen before. The cost is bounded: an unresolved slot may land on
a different Track than it would have, which the user never saw, and the
played-in-this-pass check prevents repeats. That is a limit of Spotify's API, and
it is now handled in one place instead of in each Plugin.

## Likes

Liking is one function on both services:

```rust
// Spotify
fn set_liked(conn: &Connection, item: ItemRef, liked: bool) -> Result<(), Error> {
    // Since February 2026 every save and follow goes through /me/library with a URI.
    let uri = match &item {
        ItemRef::Track(id) => format!("spotify:track:{id}"),
        ItemRef::Album(id) => format!("spotify:album:{id}"),
        ItemRef::Artist(id) => format!("spotify:artist:{id}"),
        ItemRef::User(id) => format!("spotify:user:{id}"),
        ItemRef::Playlist(id) => format!("spotify:playlist:{id}"),
    };
    let url = format!("https://api.spotify.com/v1/me/library?uris={}", enc(&uri));
    let method = if liked { Method::Put } else { Method::Delete };
    expect_2xx(conn.fetch(&with_empty_body(method, url))?)
}
```

SoundCloud's is the observed web request
`PUT|DELETE /users/{me}/track_likes/{id}` with an `Origin` header; everything
else comes from the `attach` rule. The host never retries it, so one heart tap
stays one request.

**Liked state for a page of rows.** Spotify exports `like-lookup`, and the
host calls it with up to `limits.liked-states` (40) items per request, as
legacy's `liked-states:<ids>` action did:

```rust
fn liked_states(conn: &Connection, items: Vec<ItemRef>) -> Result<Vec<bool>, Error> {
    json(conn.fetch(&get(format!("https://api.spotify.com/v1/me/library/contains?uris={}",
        items.iter().map(uri_of).collect::<Vec<_>>().join(","))))?)
}
```

SoundCloud has no batch endpoint. Legacy kept an in-Plugin index of liked ids,
filled from `/me/track_likes/ids` with five-page and ten-page budgets. Under
0.1 SoundCloud exports no `like-lookup`, and the host builds that index from
the `likes` shelf with `detail: Ids`, applying the same budgets, and updates it
after `set-liked`. The Plugin code for it is the `Detail::Ids` arm above.

Rate limits: after a 429 the host holds back requests to `api.spotify.com`
and shows that Spotify is rate-limiting, instead of legacy's three-second
toast and a one-minute back-off hard-coded in the app.

## The feed

Ristle has one feed, not one per Provider. A user following accounts on
SoundCloud and Audius sees one timeline of what they published and
reposted. That only works if every Provider describes posts the same way, so
the feed is typed data (`feed.post`), not a Provider View.

What feeds have in common, and what the record holds:

| Field | SoundCloud | Audius |
|---|---|---|
| `item` (Track, album, playlist) | yes | yes |
| `author` (who published it) | yes | yes |
| `at` | yes | yes |
| `reposted-by` (an account, linkable) | yes | yes |
| `text` (post or repost caption) | yes | yes |
| `counts` (likes, reposts, comments, plays) | yes | yes |

Liking a post is the standard like control (`likes`). Reposting is
`reposts.set-reposted`, typed like liking rather than a Plugin-defined action.
Comments are read through `comments` (see [Comments](#comments)).

**Merging.** The host asks each Provider Connection that exports `feed` for its
first page and merges by `at`, newest first. When the user scrolls past what
is loaded, it reads the next page of the Provider whose oldest post so far is
the newest, so a busy SoundCloud feed does not make Ristle page through
another Provider needlessly. Tied Providers all read their next page, since
second-resolution times and releases dated at midnight tie often.
Reposts of the same item by several accounts are grouped ("Reposted by A and
B"). Post ids keep refreshed posts in place.

**Refreshing** happens only when the user asks: pull down on mobile, the
refresh button on desktop. Nothing reloads by itself and moves rows under the
user's finger.

**SoundCloud** maps its `/stream` endpoint directly:

```rust
fn posts(conn: &Connection, cursor: Option<String>, limit: u32) -> Result<FeedPage, Error> {
    let url = match cursor {
        Some(next) => checked_v2_url(&next)?,
        None => format!("{V2}/stream?limit={}&linked_partitioning=1", limit.min(100)),
    };
    let page: Collection<V2FeedItem> = json(sc(conn, get(url))?)?;
    Ok(FeedPage { posts: page.items.iter().filter_map(post).collect(), next: page.next_href })
}

fn post(e: &V2FeedItem) -> Option<Post> {
    let (item, author) = match (&e.track, &e.playlist) {
        (Some(t), _) => (Item::Track(track(t)), artist_ref(&t.user)),
        (_, Some(p)) => (Item::Playlist(playlist(p)), artist_ref(p.user.as_ref()?)),
        _ => return None,
    };
    let repost = e.kind.contains("repost");
    Some(Post {
        id: e.uuid.clone()?,
        at: instant(&e.created_at)?,
        item,
        author,
        reposted_by: repost.then(|| e.user.as_ref().map(artist_ref)).flatten(),
        text: e.caption.clone(),
        counts: Some(Counts { likes: e.likes_count(), reposts: e.reposts_count(),
                              comments: e.comment_count(), plays: e.playback_count() }),
    })
}
```

Legacy attached "Like" and "Repost" to every feed row as Plugin actions. Both
are now standard controls, so the Plugin returns no actions at all.

```rust
fn set_reposted(conn: &Connection, item: ItemRef, reposted: bool) -> Result<(), Error> {
    let path = match item {
        ItemRef::Track(id) => format!("{V2}/me/reposts/tracks/{id}"),
        ItemRef::Playlist(id) => format!("{V2}/me/reposts/playlists/{id}"),
        _ => return Err(Error::Unsupported("Only Tracks and playlists can be reposted".into())),
    };
    expect_2xx(sc(conn, with_empty_body(if reposted { Method::Put } else { Method::Delete }, path))?)
}
```

**Spotify** has no feed and does not export `feed`.

## Provider Views

What remains a Provider View is what only one Provider has: a charts page, a
curated hub, a station list. Their pages are a title plus `section`s
(`rows`, `shelf` or `grid` over a `part`), linked from `library.views` or from
`profile-page.extras`. Neither legacy Plugin needs one any more: SoundCloud's
feed and profile drill-downs and Spotify's discography all became typed data.

## Comments

Comments are read-only in 0.1: Ristle shows them, it does not post them.
SoundCloud is the Provider that has them, and its comments are *timed*: each
is pinned to a point in the Track, which the host can show along the seek bar.

```rust
fn comments(conn: &Connection, item: ItemRef, cursor: Option<String>, limit: u32) -> Result<CommentPage, Error> {
    let ItemRef::Track(id) = item else { return Err(Error::Unsupported("Only Tracks have comments".into())) };
    let url = match cursor {
        Some(next) => checked_v2_url(&next)?,
        None => format!("{V2}/tracks/{id}/comments?threaded=1&limit={}&linked_partitioning=1", limit.min(200)),
    };
    let page: Collection<V2Comment> = json(sc(conn, get(url).ttl(60))?)?;
    Ok(CommentPage {
        comments: page.items.iter().map(|c| Comment {
            id: c.id.to_string(),
            author: artist_ref(&c.user),
            text: c.body.clone(),
            at: instant(&c.created_at).unwrap_or_default(),
            track_position_ms: c.timestamp,
            reply_to: c.parent_id.map(|p| p.to_string()),
            likes: c.likes_count,
        }).collect(),
        next: page.next_href,
    })
}
```

Spotify has no comments and does not export `comments`. Posting comments
later is a new `comment-posting` interface; reading them does not change.

## Settings

Settings that apply to every Provider, such as stream quality on mobile data,
belong to Ristle and reach Plugins as call parameters. A Plugin declares only
its own settings, and Ristle shows them on the Plugin's settings page with
its standard controls. Values are stored by the host and read with
`conn.setting(key)`, so they survive instances like everything else.

```rust
// SoundCloud
fn describe(_: &Connection) -> Result<Vec<Group>, Error> {
    Ok(vec![Group { title: None, settings: vec![
        Setting { key: "feed-reposts".into(), title: "Show reposts in the Feed".into(),
                  description: None, scope: Scope::Connection, control: Control::Toggle(true) },
        Setting { key: "hide-previews".into(), title: "Hide 30-second previews".into(),
                  description: Some("Tracks SoundCloud only lets you hear in part".into()),
                  scope: Scope::Plugin, control: Control::Toggle(true) },
    ] }])
}

fn apply(_: &Connection, _key: String, _value: SettingValue) -> Result<Option<String>, Error> {
    Ok(None)                      // nothing to validate; the host stores the value
}
```

`posts` then skips reposts when `conn.setting("feed-reposts")` is
`Some(SettingValue::Toggle(false))`. `apply` matters for settings that must be
checked or acted on, such as a button "Clear saved audio keys". A
self-hosted Provider's server is not a setting: it is chosen during sign-in
(see [Self-hosted Providers](#self-hosted-providers)), because only the user
may grant a Plugin a new destination. Turning a Queue Projection on is not a
Plugin setting: for every Plugin that exports `queue-projection`, Ristle shows
its own per-connection switch ("Show Ristle in Spotify").

## Queue Projection

A Queue Projection shows Ristle's Canonical Queue inside a Provider's own
apps and turns edits made there into Queue Mutations (CONTEXT.md). The case
that matters is **Spotify Connect**: Ristle appears in the Spotify app's
device list, the phone shows what Ristle is playing and what comes next, and
pause, skip, seek and "Add to queue" on the phone act on Ristle. Entries that
are not Spotify Tracks are shown as Projection Markers, clearly named Spotify
Tracks that hold their place, or are left out.

The Plugin never edits the Canonical Queue. It translates in both directions:

```text
core ──update(queue-state)──▶ Plugin ──PUT connect-state──▶ Spotify ──▶ phone
phone ──▶ Spotify ──dealer WebSocket──▶ host ──receive(frame)──▶ Plugin ──mutations──▶ core
```

```rust
fn start(conn: &Connection, state: QueueState) -> Result<(), Error> {
    let token = playback_token(conn)?;
    live::open(&ChannelRequest {
        url: format!("wss://dealer.spotify.com/?access_token={}", token.access_token),
        headers: vec![],
        keepalive: Some(Keepalive { every_ms: 30_000, message: r#"{"type":"ping"}"#.into() }),
    })?;
    // The device is announced once the dealer reports its connection id (`receive`).
    storage::set(&format!("{}:projection", conn.id()), &encode(&state), None)?;
    Ok(())
}

fn receive(conn: &Connection, channel: String, frame: Frame) -> Result<Mutations, Error> {
    match dealer_message(frame)? {
        Dealer::ConnectionId(id) => { announce_device(conn, &id)?; Ok(no_changes()) }
        // The host never reconnects with the old URL: its token may have expired.
        Dealer::Closed => { live::reopen(&channel, &dealer_request(conn)?)?; Ok(no_changes()) }
        Dealer::Command { revision, command } => Ok(Mutations { revision, changes: match command {
            Command::Pause => vec![Mutation::Pause],
            Command::SkipNext => vec![Mutation::Next],
            Command::SeekTo(ms) => vec![Mutation::Seek(ms)],
            Command::AddToQueue(uri) => vec![Mutation::Enqueue(vec![track_id(&uri)?])],
            Command::Transfer => vec![Mutation::Released],
            // …
        } }),
        _ => Ok(no_changes()),
    }
}

fn update(conn: &Connection, state: QueueState) -> Result<(), Error> {
    // Map each entry to a Spotify Track or a marker, then PUT the player state.
    put_connect_state(conn, &projected(&state))
}
```

The live channel is a host import (`live`) rather than a socket in the
Plugin, because a phone can send a command at any moment, also while no
Plugin call is running. The host keeps the WebSocket open, sends the
keep-alive, and hands each frame to whichever instance it calls. Reconnecting
goes through the Plugin (`live.reopen`), so a refreshed token is used instead
of the one the channel was first opened with.

The `mutations.revision` is the queue revision the phone was looking at. If
the queue has moved on since, the core applies a mutation only where it still
makes sense (a pause always does; removing an entry that is already gone does
not), which is the same rule Party Commands follow.

SoundCloud has no API for its app's queue and exports no projection. The
exact Connect messages above follow librespot's implementation and are part
of spike E.

## Party and Provider terms

Party State relays Track metadata to guests, and a guest's search or browse
relays albums, profiles and playlists. By default every field of `track`,
`album`, `profile` and `playlist` may be relayed; nothing from a Credential
Record, a Stream Source or `storage` ever is. A Plugin narrows this with
`[party] share` in its manifest, which applies to all four: a listed name
covers that field in every one of them that has it, and an item's reference
is always relayed. Only those four records are relayed. What wraps them
(a listing entry's `added` and `added-by`, a page's parts and totals, a feed
post's author and time) never reaches guests, whatever `share` says.

Spotify's Developer Terms and Developer Policy restrict sharing content with
third parties, playing to several listeners, and mixing Spotify with other
services. ADR 0012 records how Ristle reads them and what it does, in short:

- **Mixing** means blending audio or passing one service off as another. A
  queue that holds Spotify Tracks next to other Providers' Tracks does not mix
  them as long as the audio never overlaps and every Track is shown as its
  Provider's. The Spotify manifest sets `[playback] overlap = false`, so the
  host never crossfades into or out of a Spotify Track. The attribution is
  the Provider icon or text badge Ristle's UI rules already put on every
  Track; nothing extra is added.
- **Third parties.** The Spotify manifest sets `[party] share = []`, so the
  Party Host relays only references, for Tracks and for any album, artist or
  playlist a guest's search returns. A guest whose Ristle has a Spotify
  Provider Connection resolves the metadata with their own account; any other
  guest sees that a Spotify item is there, without its details.
- **Storage.** The host's cache of Provider data is bounded and refreshed by
  user action, never kept indefinitely.

These measures are made out of respect, not as a legal position. Spotify can
stop the Spotify Plugin whatever Ristle does.

## Streams

### SoundCloud: HLS

```rust
fn resolve(conn: &Connection, id: String, quality: Quality) -> Result<StreamSource, Error> {
    let t: V2Track = json(sc(conn, get(format!("{V2}/tracks/{id}")).ttl(600))?)?;
    if let Some(reason) = unplayable_reason(&t) { return Err(Error::Unavailable(reason.into())); }
    let auth = t.track_authorization.as_deref().ok_or_else(|| Error::Unavailable("No stream".into()))?;
    for tc in modern_aac_hls(&t, quality) {                    // legacy's ranking, unchanged
        match sc(conn, get(format!("{}?track_authorization={auth}", tc.url))).and_then(json::<StreamUrl>) {
            Ok(s) => return Ok(StreamSource {
                location: Location::Hls(Hls { url: s.url, headers: vec![] }),
                mime: Some(tc.format.mime_type.clone()),
                duration_ms: t.duration,
                expires_at: Some(system::now() + 5 * 60_000),
            }),
            Err(Error::Transient(m)) => return Err(Error::Transient(m)),  // never "unavailable"
            Err(_) => continue,
        }
    }
    Err(Error::Unavailable("SoundCloud offers no AAC stream for this track".into()))
}
```

Legacy's fallback to the public `/tracks/{id}/streams` endpoint and its "refresh
the Track once if the page payload was stale" step stay; they are omitted
here. The 60-second stream cache becomes the HTTP cache on the
`track_authorization` request. The HLS media hosts must be in
`media-hosts`, because the host fetches them for the Plugin.

### Spotify: encrypted progressive, decrypted by the host

Legacy Spotify ran librespot inside the Plugin and delivered PCM, which ADR
0004 rules out. What librespot does underneath is simple enough to express
as a Stream Source:

1. Track metadata from spclient (`/metadata/4/track/{gid}`, protobuf) lists
   the audio files per format (Ogg Vorbis 96/160/320).
2. `storage-resolve` on spclient returns signed CDN URLs for the file.
3. The file's AES-128 key comes from the access point, a TCP connection with
   Spotify's own handshake and Shannon framing.
4. The file is AES-128-CTR with a fixed IV over the whole file, and the
   plaintext starts with a 167-byte Spotify header before the Ogg stream.

So:

```rust
const AUDIO_IV: [u8; 16] = hex!("72e067fbddcbcf77ebe8bc643f630d93");

fn resolve(conn: &Connection, id: String, quality: Quality) -> Result<StreamSource, Error> {
    let playback = playback_token(conn)?;                         // from credential.custom
    let meta = spclient_track(conn, &playback, &id)?;             // protobuf decode
    let file = meta.pick_vorbis(quality).ok_or_else(|| Error::Unavailable("No Ogg Vorbis file".into()))?;
    let cdn: StorageResolve = json(conn.fetch(&get(format!(
        "https://spclient.wg.spotify.com/storage-resolve/v2/files/audio/interactive/11/{}?alt=json", file.hex()))
        .header("Authorization", format!("Bearer {}", playback.access_token)))?)?;
    let key = audio_key(conn, &playback, &meta.gid, &file.id)?;    // storage first, then the access point
    Ok(StreamSource {
        location: Location::Progressive(Progressive {
            url: cdn.cdnurl[0].clone(), headers: vec![],
            decrypt: Some(Decryption { mode: AesMode::Ctr, key, iv: AUDIO_IV.to_vec(), skip: 167 }),
        }),
        mime: Some("audio/ogg; codecs=vorbis".into()),
        duration_ms: Some(meta.duration_ms),
        expires_at: cdn.expires_at(),
    })
}

fn audio_key(conn: &Connection, playback: &Token, gid: &[u8], file: &[u8]) -> Result<Vec<u8>, Error> {
    let cache_key = format!("key:{}", hex(file));
    if let Some(key) = storage::get(&cache_key) { return Ok(key); }
    let mut ap = access_point(playback)?;          // socket::Tcp, kept in instance memory
    let key = ap.request_key(gid, file)?;          // one packet out, one in
    storage::set(&cache_key, &key, None)?;
    Ok(key)
}
```

The host Range-requests the CDN file, computes the CTR counter for any
offset, decrypts, drops the first 167 bytes and demuxes Ogg Vorbis with
Symphonia. Seeking is the host's normal Ogg seek. The Plugin spends no CPU on
audio, which matters on iOS, where Plugins run about 24× slower than native.

**What the access point is, and what statelessness costs.** Spotify's own
clients keep one long-lived TCP connection to an "access point"
(`ap-*.spotify.com`, port 4070 or 443) with Spotify's own encryption. It
predates their HTTPS APIs, and some things are still only available there,
among them the keys that decrypt audio files. librespot, and therefore
ncspot and Ristle legacy, open it once per session.

Under 0.1 the connection lives in the memory of one Plugin instance, because
sockets cannot be stored. When the host discards that instance, the next
audio key needs a new connection. That costs:

- **Time, not a sign-in.** Reconnecting is a TCP and crypto handshake plus a
  login with the stored playback token, about 0.3–1 s. The user is not asked
  for anything, and no credential is lost.
- **No cache.** Audio keys are in `storage`, not in the instance, so they
  survive. Only Tracks never played before need the access point at all, and
  the key for the next Track is fetched while the current one plays.
- **Some extra logins.** Each reconnection is a login from Spotify's point of
  view. The host limits this by keeping an instance alive while it is in use
  (during playback, and for a few minutes after the last call) instead of
  discarding instances eagerly. Web API calls do not go through the access
  point, so they are unaffected; their rate limits are the playback client's
  existing problem, handled by the host's back-off.

Two parts of this are unverified and decide whether the Spotify Plugin works
at all (see the decisions):

- The access point needs raw TCP, hence the `socket` import. Its handshake
  needs a 768-bit Diffie-Hellman exchange in the Plugin, once per instance.
  That is plain big-integer arithmetic and should cost tens of milliseconds
  under Pulley, but it has not been measured.
- spclient needs a client token (`clienttoken.spotify.com`) and accepts the
  playback client's OAuth token. librespot obtains both today; whether it can
  do so over plain HTTPS without its session is what the Spotify port has to
  confirm first.

Spotify's normalization gain is not used, as in legacy, which disabled
librespot's normalization. Ristle plays every Provider at the level its file
has (legacy ADR 0008).

### When a Byte Source is needed

Neither Plugin needs `byte-source`. It exists for CDNs whose bytes cannot be
described as one HTTP resource plus a standard cipher, such as files split
into chunks that are each fetched separately, or a cipher the host does not
offer. The Plugin then returns `Location::Bytes` with a handle that encodes
everything needed (for example the chunk URL template and key), and the host
calls `read(handle, offset, length)` ahead of the playhead.

## Spotify's Web API in Development Mode

The Spotify sketches above use the Web API as legacy did. Since Spotify's
February 2026 changes (applied to existing Development Mode apps on 9 March
2026), an app without extended quota, which is what a third-party Publisher's
catalog client is, lost several endpoints the legacy Plugin relied on:

| Removed for Development Mode | Used here for | Where it goes instead |
|---|---|---|
| `GET /tracks?ids=` (several Tracks) | `tracks` | spclient metadata through the playback client, or `GET /tracks/{id}` per id with `fetch_all` |
| `GET /artists/{id}/top-tracks` | artist `top-tracks` | spclient artist metadata |
| `GET /users/{id}`, `GET /users/{id}/playlists` | user pages | spclient (user profile and rootlist) |
| Items of other users' playlists (`/playlists/{id}/items` now only for owned playlists) | playlist pages | spclient `playlist/v2` |
| `PUT/DELETE /me/following`, `/playlists/{id}/followers`, `/me/tracks`, `/me/albums` | `set-liked` | `PUT/DELETE /me/library?uris=` (done above) |
| `GET /me/tracks/contains` and other `contains` | `like-lookup` | `GET /me/library/contains?uris=` (already used) |
| Search `limit` above 10 | `search` | The Plugin clamps to 10 per group |
| `/playlists/{id}/tracks` | playlist reads and edits | renamed `/playlists/{id}/items` (done above) |

Nothing here changes the WIT: the exports stay the same, and only where the
Spotify Plugin gets its data moves to spclient, which it already needs for
playback. Which spclient endpoints serve these, and at what batch sizes, is
part of spike E. Sources: [February 2026 changelog](https://developer.spotify.com/documentation/web-api/references/changes/february-2026),
[migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide).

## Self-hosted Providers

Jellyfin, Plex, Navidrome and other servers the user runs are Providers like
any other, but their address is not known when the Plugin is packaged, and
it is often on the home network. A manifest cannot list it, and the public
address rule would refuse it. A **Server Grant** solves this: the manifest
only says the Plugin may ask (`servers = true`), and the user grants one
server to one Provider Connection during sign-in.

```rust
// Jellyfin
fn begin_authorization(_: &Connection, _: SessionContext) -> Result<Step, Error> {
    Ok(ask(Challenge::Server(ServerChoice {
        found: vec![], allow_entry: true,
        placeholder: Some("http://192.168.1.20:8096".into()) }), "server"))
}

fn continue_authorization(conn: &Connection, _: SessionContext, state: String, answer: Answer)
        -> Result<Step, Error> {
    match (state.as_str(), answer) {
        ("server", Answer::Server(_)) => Ok(ask(Challenge::Form(login_form()), "login")),
        ("login", Answer::Form(values)) => {
            let server = conn.server().ok_or_else(unauthorized)?;   // granted from here on
            let base = &server.urls[0];                       // may carry a path
            let auth: JfAuth = json(conn.fetch(&with_json(Method::Post,
                format!("{base}/Users/AuthenticateByName"), login_body(&values)))?)?;
            let credential = token_credential(&server, &auth.access_token);
            conn.set_credential(&credential)?;
            Ok(Step::Done(Authorized { credential, account: auth.user.into() }))
        }
        _ => Err(Error::Internal("unexpected step".into())),
    }
}
```

Plex discovers instead of asking for an address: the Plugin signs in to
plex.tv (a manifest host), lists the account's servers, and returns them as
`found`, each with its local and remote URLs. The user picks one.

What the user sees and what the host enforces:

- **At install**, one line: "Connects to servers you choose, including ones
  on your home network". Nothing is reachable yet.
- **At sign-in**, the server and every origin it would grant. A local
  origin adds "This server is on your home network" (or "This server runs
  on this device") and an `http` origin adds "Traffic to this server is not
  encrypted", each as a confirmation the user must accept. A Plugin cannot
  add origins later; changing servers is signing in again, which asks
  again, and like a different account it drops everything cached for the
  connection.
- **Confirmed addresses stick.** The grant remembers, for each origin,
  that it was public, or the exact home-network or same-device address the
  user confirmed. If a name later resolves anywhere else, for example a
  public name rebound to a home address, or `nas.local` pointing at a
  different device, the host refuses the connection and asks the user
  whether the server at the new address is still theirs. DNS rebinding
  cannot skip the warning or steer requests to another device; the cost is
  one confirmation when a home server's address really changes.
- **Items stay with their server.** Two Jellyfin servers can issue the
  same id for different Tracks, so the host keeps the Provider Connection
  with every item of a `servers = true` Plugin and never resolves it
  through another connection.
- **The grant** belongs to that Provider Connection. Its origins count as
  `hosts`, `media-hosts` and `image-hosts` for that connection only, so
  streams and artwork from the server may be plain HTTP too, and may resolve
  to local addresses, and on desktop to this device. Every other destination keeps the public-HTTPS rule. Removing the
  connection removes the grant.
- **Platforms.** iOS shows its Local Network permission the first time a
  granted origin is local, so the app declares
  `NSLocalNetworkUsageDescription`. Android must allow cleartext traffic in
  its network security config, since granted addresses are not known in
  advance; the host's own rule keeps everything else HTTPS.

**Developer mode.** A Ristle setting for Plugin authors lets a sideloaded
Plugin's manifest hosts resolve to local addresses and use HTTP, so a Plugin
can be tested against a server on the developer's machine. Ristle shows a
banner while it is on, and it never applies to Plugins installed from a
feed.

## Errors and rate limits

Every legacy string error that the host had to classify becomes a case:

| Legacy | 0.1 |
|---|---|
| "SoundCloud sign-in expired. Connect SoundCloud again." | `unauthorized` |
| "Spotify search is rate limited. Retry after N seconds." | `rate-limited(Some(n))`, produced by the host |
| "SoundCloud did not provide a current AAC stream" | `unavailable` |
| reqwest connection error | `transient`, produced by the host |
| "Spotify Playlist changed while its sequence was enumerated" | not needed: the host compares revisions |

## Verbosity check

Counted on the sketches above, with the mapping functions (`track`, `album`,
`profile`, `playlist`) counted once per Plugin:

| | Legacy SoundCloud | 0.1 SoundCloud (estimate) | Legacy Spotify | 0.1 Spotify (estimate) |
|---|---|---|---|---|
| Plugin total | ~7,300 lines | ~2,000 | ~5,100 (+librespot) | ~2,000 (+access point, ~600) |
| Sign-in | ~900 | ~120 | ~550 | ~120 |
| One browse surface (e.g. an artist page) | ~150 | ~50 | ~120 | ~50 |

Most of the reduction is state, caching and error classification moving into
the host, and the sequence API merging into `list`. The exports a minimal
keyless Plugin has to write are `search`, `my-library`, `open`, `list`,
`tracks` and `resolve`. Six functions is one more than legacy's quickstart,
because `tracks` is required for hydration; `my-library` and `open` may return
almost nothing.

Where the WIT is still wordy: generated records have no defaults, so a
`Track` or `ProfilePage` literal lists every field. The SDK should derive `Default`
where the bindings allow it and offer constructors such as
`Entry::added(item, at)`. That is SDK work, not a WIT change.

## Refreshing and background calls

Ristle refreshes only when the user asks. Content that moves by itself causes
misclicks and spends rate limits.

- **Feed, pages, library:** pull down on mobile, a refresh button on desktop.
  Opening a page shows what is cached; it fetches only when nothing is cached
  or the cache has expired, and never swaps content after showing it.
- **Likes and playlists:** the same. Refreshing the library re-lists its
  shelves and compares revisions, so unchanged playlists cost one request.
  Changes made in Ristle are applied locally right away and need no refresh.
- **During a refresh** the host revalidates the cached HTTP responses the
  Plugin reads, so Plugins have no refresh flag. A rate-limited refresh keeps
  showing the cached data (`cache-override.stale-seconds`) with a notice.

Background calls are calls the user made from outside the app's screens: a
like from the lock screen or a notification, Android Auto browsing the
library and starting a playlist, a Party guest's search or request reaching
the host. They are ordinary Plugin calls, made while the app is in the
background, and the Plugin cannot tell the difference. They belong to the
phases that build those surfaces (Android in phase 3, Party in phase 5), not
to a separate scheduler. There are no timed or periodic Plugin calls.

## What the walkthrough changed

Each of these came from a mapping that read badly in an earlier draft, or
from review:

1. **Sequences merged into collections.** Both legacy Plugins implement every
   playlist, album and artist list twice (`*_page` and
   `playable_sequence_page` with the same request). Now there is one `list`,
   with `detail` choosing between full items and cheap ids.
2. **Positions are synthesized by the host.** An earlier draft had the Plugin
   return position handles. Spotify's Liked Songs needed thousands of them for
   nothing; `addressing: offset` plus a total lets the host make them.
3. **Credentials attach themselves.** Every SoundCloud request needed eight
   headers and query parameters from the token. `credential.attach` states
   them once, and refreshing replaces them.
4. **Capabilities are exports.** Stub functions returning `unsupported`
   cluttered both Plugins. Optional interfaces removed the stubs and the
   capability flags.
5. **No `connect` export.** Legacy's `ConfigureConnection` validated
   credentials and fetched the account label on every start. The host holds
   the account from sign-in and only calls `refresh-credential` when needed.
6. **Liked state can be host-derived,** which removed SoundCloud's most
   intricate in-Plugin cache.
7. **Pages are typed data** (review). Generic sections were a layout language
   in disguise. Album, playlist and profile pages are now records the host
   lays out the same way for every Provider; sections remain only in
   Provider Views.
8. **The feed is first class** (review). One merged feed across Providers,
   from typed posts with a linkable author and reposter, instead of a
   SoundCloud-only Provider View.
9. **Reposts are typed and Plugin actions are gone** (review). Like and
   repost were the only row actions either Plugin had.
10. **Accounts are `artist` or `user`** (review). Spotify keeps them apart, so
    links to accounts are a `person` variant.
11. **Drift never reshuffles** (review). Changes to an offset collection are
    applied to what has not been decided yet, never to what the user sees.
12. **No background revalidation** (review). Stale-while-revalidate became
    stale-if-error; fetching again is the user's decision.
13. **Settings, read-only comments and Queue Projection** (second review),
    built now because they are known to be needed. Queue Projection added
    the `live` import: the host keeps a Provider's WebSocket open so remote
    commands reach the Plugin without a running call.

## Decisions

Settled in review (2026-10-12):

- **Raw TCP (`socket`)** is accepted. Only Spotify needs it.
- **Web capture stays declarative.** Revisit when SoundCloud testing starts
  and more Plugins need web capture.
- **Plugins are stateless between calls,** and My Library Search is
  host-side.
- **Provider Views keep rows, shelves and grids** for now, with `layout` as a
  variant so new kinds can be added.
- **Partial writes are not atomic.** The host chunks by manifest limits and
  reports per chunk; Plugins report refused items in `outcome.rejected`.
- **No timed background work.** Refreshes are user-triggered; background
  calls are user actions from the lock screen, Android Auto or a Party.
- **Media-host wildcards** are acceptable.
- **Versioning:** `0.x` until the first third-party Plugin ships, then `1.0`.
  New fields mean a new package version; the host implements every `1.x`
  world side by side and keeps the previous major for 12 months.
- **Party metadata:** `track`, `album`, `profile` and `playlist` fields may
  be relayed to guests, never anything from `credential`, `stream-source` or
  `storage`. A Plugin narrows this in its manifest (`[party] share`, which
  covers all four item types). The Spotify Plugin relays only references,
  and guests resolve them with their own Spotify account. ADR 0012 records Ristle's reading of Spotify's
  terms, including no crossfade at Spotify boundaries.
- **A Spotify spike** confirms the access point and spclient paths before the
  WIT is final. Its brief is in `HANDOFF.md`.

Settled in the second review:

- **No Spotify feed.** Spotify has no feed; the Plugin does not build one.
- **Comments are read-only** in 0.1 (`comments`).
- **No playback gain** (`gain-db` removed), as in legacy.
- **Settings and Queue Projection are in 0.1**, not deferred: `settings`
  for Plugin-specific settings, `queue-projection` with host-kept `live`
  channels for Spotify Connect.
- **Playlist editing on both Plugins.** SoundCloud's web-session requests
  still have to be observed and recorded.
- **Device-code sign-in stays.**

Settled after the Codex reviews (2026-10-13):

- **Self-hosted Providers are in 0.1** through Server Grants: the user
  grants one server to one Provider Connection during sign-in, with
  explicit confirmations for local and unencrypted origins.
- **Playlist write races are accepted where the Provider has no
  conditional write:** SoundCloud's whole-list `PUT` (unless it honours
  `If-Unmodified-Since`) and the moment around a Spotify positional add.
  Both editing paths stay.

Principle from that review: build what is known to be needed into the first
version instead of retrofitting it.

Still open:

1. **The Spotify Connect messages** in [Queue Projection](#queue-projection)
   follow librespot and are unverified. Spike E covers them along with the
   access point.
