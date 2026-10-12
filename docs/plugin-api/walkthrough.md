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

The code is Rust against the types `wit-bindgen` generates. It assumes five
helpers that the Ristle SDK will provide, each a few lines over the generated
types:

```rust
fn get(url: impl Into<String>) -> Request;      // GET, no headers, no body, cache: Http
impl Request {
    fn header(self, name: &str, value: impl Into<String>) -> Self;
    fn ttl(self, seconds: u32) -> Self;          // cache: Override { ttl, stale: ttl, key: None }
}
fn json<T: DeserializeOwned>(r: Response) -> Result<T, Error>;
// 2xx → T; 404/410 → Unavailable; 5xx → Transient; other → Internal(status + body excerpt)
impl From<Item> for Entry { /* note: None */ }
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
    export ristle:plugin/actions@0.1.0;
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
}
```

SoundCloud does not export `like-lookup`: the host answers liked state from
its liked-tracks shelf (see [Likes](#likes)). Neither exports `byte-source`.

The static facts (id, hosts, redirects, batch limits) live in the manifest,
described in `packaging.md`. The parts that matter here:

```toml
# SoundCloud
[permissions]
hosts = ["api-v2.soundcloud.com", "api.soundcloud.com", "secure.soundcloud.com"]
media-hosts = ["*.sndcdn.com", "*.media-streaming.soundcloud.cloud"]
web-capture = true
[auth]
redirects = ["loopback:8990/login", "scheme", "remote"]
[limits]
tracks = 50

# Spotify
[permissions]
hosts = ["api.spotify.com", "accounts.spotify.com", "spclient.wg.spotify.com",
         "apresolve.spotify.com", "clienttoken.spotify.com"]
media-hosts = ["*.scdn.co", "*.spotifycdn.com", "audio-ak-spotify-com.akamaized.net"]
sockets = ["*.spotify.com:4070", "*.spotify.com:443"]
[auth]
redirects = ["loopback:8989/login", "scheme"]
[limits]
tracks = 50
liked-states = 40
playlist-add = 100
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
    conn.set_credential(&credential);                 // so the next fetch is signed
    let me: V2Me = json(conn.fetch(&get("https://api-v2.soundcloud.com/me"))?)?;
    Ok(Step::Done(Authorized { credential, account: me.into() }))
}
```

`query` is appended only when the URL lacks the parameter, which replaces
legacy's `web_request_builder_with_existing_context` that checked `next_href`
cursors for an existing `client_id`.

**DataDome rotation.** SoundCloud refreshes its DataDome value in an
`x-set-cookie` response header and expects it on the next request. The Plugin
keeps one wrapper for that:

```rust
fn sc(conn: &Connection, req: Request) -> Result<Response, Error> {
    let response = conn.fetch(&req)?;
    if let Some(fresh) = datadome_from_x_set_cookie(&response.headers) {
        let mut c = conn.credential().ok_or_else(unauthorized)?;
        set_attached_header(&mut c, "x-datadome-clientid", fresh);
        conn.set_credential(&c);
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
user before replacing anything.

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
    conn.set_credential(&credential);
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

## Search

Both Plugins answer one call per connection. The host asks for every group at
once, then for one group with a cursor for "Show more".

SoundCloud has one endpoint per group, so it fans out with `fetch_all`, like
legacy's `send_batch`:

```rust
fn search(conn: &Connection, req: SearchRequest) -> Result<SearchResults, Error> {
    if let Some(next) = req.cursor {                       // one group, next page
        let page: Collection<Any> = json(sc(conn, get(checked_v2_url(&next)?))?)?;
        return Ok(SearchResults { items: page.items(), next: page.next_href });
    }
    let groups = [(Kinds::TRACKS, "tracks"), (Kinds::ARTISTS, "users"), (Kinds::PLAYLISTS, "playlists")];
    let asked: Vec<_> = groups.iter().filter(|(k, _)| req.kinds.contains(*k)).collect();
    let requests = asked.iter().map(|(_, path)| get(format!(
        "https://api-v2.soundcloud.com/search/{path}?q={}&limit={}&linked_partitioning=1",
        enc(&req.query), req.limit)).ttl(60)).collect::<Vec<_>>();
    let mut items = Vec::new();
    let mut next = None;
    for response in conn.fetch_all(&requests) {
        let page: Collection<Any> = json(response?)?;
        if asked.len() == 1 { next = page.next_href.clone(); }
        items.extend(page.items());
    }
    Ok(SearchResults { items, next })
}
```

Spotify has one endpoint with `type=track,album,artist,playlist`. Its
`next` URLs are the cursor. The one-hour search cache in legacy becomes
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

Spotify's is the same against `/v1/tracks?ids=`, which answers `null` for a
missing id. Neither Plugin chunks: the host never sends more than 50.

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

## Albums

**Spotify** albums are albums. `open(Target::Album(id))` returns the album as
the subject and one track section:

```rust
fn open_album(conn: &Connection, id: &str) -> Result<Page, Error> {
    let a: AlbumDetail = json(conn.fetch(&get(format!("https://api.spotify.com/v1/albums/{id}")))?)?;
    let album = album(&a);
    let tracks = a.tracks.items.iter().map(|t| Item::Track(simplified_track(t, &album)).into()).collect();
    Ok(Page {
        subject: Subject::Album(album),
        sections: vec![Section {
            title: None, layout: Layout::Rows, entries: tracks,
            more: a.tracks.next.map(|next| More {
                collection: Collection { id: format!("album:{id}"), addressing: Addressing::Offset,
                    total: Some(Count::Exact(a.tracks.total)), revision: None, endless: false },
                next: None }),
        }],
        play: None,
    })
}
```

Albums longer than one page continue through `list("album:{id}", offset)`.
The `album_type` maps to `release-type` as in legacy (`album`, `single`,
`compilation`; a single with more than three Tracks is an `ep`).

**SoundCloud** has no album entity: albums, EPs and singles are playlists
("sets") with a `set_type`. The Plugin returns them as `Item::Album` where the
set type says so, with the `set_type` mapped onto `release-type`, and as
`Item::Playlist` otherwise. `open(Target::Album(id))` uses the same
`/playlists/{id}` code as a playlist (see below), with `Subject::Album`.

## Artists and profiles

An artist page is a subject plus sections. Legacy's `top_tracks`, `albums`,
`releases_link` and `profile_modules` fields map onto ordinary sections, so
the host renders them with the same code as everything else.

**SoundCloud** users are artists:

```rust
fn open_artist(conn: &Connection, id: &str) -> Result<Page, Error> {
    let [user, tracks, albums] = fetch3(conn, [
        get(format!("{V2}/users/{id}")).ttl(300),
        get(format!("{V2}/users/{id}/tracks?limit=20&linked_partitioning=1")).ttl(300),
        get(format!("{V2}/users/{id}/albums?limit=12&linked_partitioning=1")).ttl(300),
    ])?;
    let user: V2User = json(user)?;
    let tracks: Collection<V2Track> = json(tracks)?;
    let albums: Collection<V2Playlist> = json(albums)?;
    let link = |view: &str, title: &str, count: Option<u64>| Item::Link(Link {
        to: ViewRef { view: view.into(), params: id.into() }, title: title.into(),
        subtitle: count.map(|n| format!("{n}")), artwork: vec![] }).into();
    Ok(Page {
        subject: Subject::Artist(profile(&user)),
        sections: vec![
            Section { title: Some("Tracks".into()), layout: Layout::Rows,
                      entries: tracks.items.iter().map(|t| Item::Track(track(t)).into()).collect(),
                      more: tracks.next_href.map(|n| More { collection: cursor_collection(format!("artist-tracks:{id}")), next: Some(n) }) },
            Section { title: Some("Releases".into()), layout: Layout::Shelf,
                      entries: albums.items.iter().map(|p| Item::Album(album(p)).into()).collect(),
                      more: None },
            Section { title: None, layout: Layout::Rows, more: None, entries: vec![
                link("discography", "Discography", None),
                link("profile-playlists", "Playlists on their profile", user.playlist_count),
                link("profile-likes", "Tracks they've liked", user.likes_count),
                link("profile-reposts", "Reposts", None),
            ] },
        ],
        play: Some(cursor_collection(format!("artist-discography:{id}"))),
    })
}
```

`play` names the collection Play and Shuffle use, which replaces legacy's
`top_tracks_sequence` and the page-level `playable_sequence`. Its listing is
legacy's `artist-discography` sequence: release Tracks first, then the uploads
that were never part of a set. The phase lives in the opaque `next` token,
exactly as legacy's `DiscographySequenceCursor` did, and with
`detail: Ids` it returns `Item::TrackId` entries so that the host can
enumerate a large discography without fetching full Tracks.

The drill-down views (`discography`, `profile-likes`, ...) are `open` with
`Target::View` and return a `Subject::View` heading plus one section each.

**Spotify** artists have the same shape: popular Tracks (`/top-tracks`, no
`more`), a releases shelf (`include_groups=album,single`), and a Discography
link. Spotify users (playlist owners) are `Target::User`, whose page holds
their public playlists.

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
is an exact revision:

```rust
fn list_playlist(conn: &Connection, id: &str, req: ListRequest) -> Result<Listing, Error> {
    let offset = match req.cursor { Cursor::Offset(o) => o, _ => 0 };
    let fields = match req.detail {           // cheap enumeration when the host only needs ids
        Detail::Ids => "&fields=total,snapshot_id,items(track(id))",
        Detail::Items => "",
    };
    let page: PlaylistTracks = json(conn.fetch(&get(format!(
        "https://api.spotify.com/v1/playlists/{id}/tracks?offset={offset}&limit=100{fields}")))?)?;
    Ok(Listing {
        entries: page.items.into_iter().map(|i| match i.track {
            Some(t) if req.detail == Detail::Ids => Item::TrackId(t.id).into(),
            Some(t) => Entry { item: Item::Track(track(&t)), note: added(&i) },
            None => Item::Unavailable(Some("No longer on Spotify".into())).into(),
        }).collect(),
        next: None,
        total: Some(Count::Exact(page.total)),
        revision: page.snapshot_id,
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

Spotify maps one-to-one:

```rust
fn edit(conn: &Connection, id: String, revision: Option<String>, change: Change) -> Result<Outcome, Error> {
    let url = format!("https://api.spotify.com/v1/playlists/{id}/tracks");
    let body = match change {
        Change::Add(i) => json!({ "uris": uris(&i.tracks), "position": i.at }),
        Change::Remove(o) => json!({ "snapshot_id": revision,
            "tracks": o.iter().map(|o| json!({ "uri": uri(&o.track), "positions": [o.position] })).collect::<Vec<_>>() }),
        Change::Move(m) => json!({ "snapshot_id": revision, "range_start": m.from,
                                   "range_length": m.count, "insert_before": m.to }),
        Change::Details(d) => return details(conn, &id, d),
    };
    let method = match change { Change::Add(_) => Method::Post, Change::Remove(_) => Method::Delete, _ => Method::Put };
    let r: Snapshot = json(conn.fetch(&with_json(method, url, body))?)?;
    Ok(Outcome { revision: Some(r.snapshot_id), rejected: vec![] })
}
```

The host splits a 250-Track Playlist Add into chunks of `limits.playlist-add`
(100), calls `edit` three times, and reports "Added 200 of 250" if the third
chunk fails. `rejected` is for Providers that refuse individual items inside
a request that otherwise succeeded.

SoundCloud's API replaces a playlist's whole Track list in one `PUT`, so its
`edit` reads the current list, applies the change, and writes it back. If
`last_modified` no longer matches `revision`, it returns `error.conflict`.
Legacy declared `playlist_write: false` for SoundCloud; with this mapping it
could support writes, but the web-session `PUT` has not been observed yet and
must be measured like the favorites requests were.

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
        views: vec![Link { to: ViewRef { view: "feed".into(), params: String::new() },
                           title: "Feed".into(), subtitle: None, artwork: vec![] }],
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
                note: Some(Note { verb: Some(Verb::Liked), at: instant(created_at),
                                  actor: None, caption: None, actions: vec![] }) },
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
            note: Some(liked_at(&s.added_at)) }).collect(),
        next: None,
        total: Some(Count::Exact(page.total)),
        // Spotify has no snapshot for Liked Songs. The total is a weak signal.
        revision: Some(page.total.to_string()),
    })
}
```

The host does the rest, which is where the safety belongs:

1. To shuffle 5,000 Liked Songs, it reads offset 0 once, learns the total,
   and shuffles the positions 0..5000 without fetching them.
2. Before playing position 3,117, it fetches the page at offset 3,100 (one
   request covers the 17 neighbors it will need next), plays the Track it
   actually got, and keeps the Track id. A position is never played without
   first being read.
3. If the page's `revision` differs from the one the shuffle was built on,
   or a re-read page disagrees with Track ids it already knows for the same
   slots, every position from the old revision is void. The host rebuilds the
   remaining order from what it has already played, instead of mapping old
   positions onto new neighbors.
4. A like or unlike made in Ristle voids the revision immediately.

Legacy's saved-tracks page cache in the Plugin is gone; the host keeps fetched
pages for the life of the shuffle.

What 0.1 cannot fix: an unlike and a like made elsewhere between two reads
keep the total equal and shift positions. Step 3's overlap check catches it
only on pages the host has seen before. That is a limit of Spotify's API, and
it is now handled in one place instead of in each Plugin.

## Likes

Liking is one function on both services:

```rust
// Spotify
fn set_liked(conn: &Connection, item: ItemRef, liked: bool) -> Result<(), Error> {
    let url = match &item {
        ItemRef::Track(id) => format!("https://api.spotify.com/v1/me/library?uris=spotify:track:{id}"),
        ItemRef::Album(id) => format!("https://api.spotify.com/v1/me/library?uris=spotify:album:{id}"),
        ItemRef::Artist(id) => format!("https://api.spotify.com/v1/me/following?type=artist&ids={id}"),
        ItemRef::Playlist(id) => format!("https://api.spotify.com/v1/playlists/{id}/followers"),
        ItemRef::User(id) => format!("https://api.spotify.com/v1/me/following?type=user&ids={id}"),
    };
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

## Feeds and Provider Views

The SoundCloud feed is a Provider View whose page is one `rows` section over
an endless collection. Each event is an entry with a note:

```rust
fn feed_entry(e: &V2FeedItem) -> Option<Entry> {
    let (item, urn) = match (&e.track, &e.playlist) {
        (Some(t), _) => (Item::Track(track(t)), t.urn.clone()),
        (_, Some(p)) => (Item::Playlist(playlist(p, false)), format!("soundcloud:playlists:{}", p.id)),
        _ => return None,
    };
    Some(Entry { item, note: Some(Note {
        actor: e.user.as_ref().map(profile),
        verb: Some(if e.kind.contains("repost") { Verb::Reposted } else { Verb::Posted }),
        at: instant(&e.created_at),
        caption: None,
        actions: vec![Action { id: format!("repost:{urn}"), label: "Repost".into(), confirm: false }],
    }) })
}
```

Legacy attached both "Like" and "Repost" as plugin actions. "Like" is now the
host's standard like control, so only Repost remains, and `actions.invoke`
handles it:

```rust
fn invoke(conn: &Connection, action: String) -> Result<Option<String>, Error> {
    let urn = action.strip_prefix("repost:").ok_or_else(|| Error::Unsupported(action.clone()))?;
    expect_2xx(sc(conn, with_empty_body(Method::Put, format!("{V2}/me/reposts/{}", checked_urn(urn)?)))?)?;
    Ok(Some("Reposted".into()))
}
```

Legacy's `refresh_token` on the feed page and the 30-second feed cache become
`.ttl(30)` on the `/stream` request; "pull to refresh" asks the host to
re-list from `start`, and the host's stale-while-revalidate does the rest.

Legacy protocol 1.9 also had chart, station, article and external-link
sections. Neither Plugin used them, and they are not in 0.1 (see the
decisions).

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
                duration_ms: t.duration, gain_db: None,
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
        gain_db: None,
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

`gain-db` could carry Spotify's normalization gain, which sits in the
167-byte header. Reading it costs one extra Range request and a 167-byte
decrypt. Legacy did not normalize (legacy ADR 0008), so 0.1 leaves it out of
the Spotify mapping.

### When a Byte Source is needed

Neither Plugin needs `byte-source`. It exists for CDNs whose bytes cannot be
described as one HTTP resource plus a standard cipher, such as files split
into chunks that are each fetched separately, or a cipher the host does not
offer. The Plugin then returns `Location::Bytes` with a handle that encodes
everything needed (for example the chunk URL template and key), and the host
calls `read(handle, offset, length)` ahead of the playhead.

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
`Track` or `Note` literal lists every field. The SDK should derive `Default`
where the bindings allow it and offer constructors such as
`Note::liked(at)`. That is SDK work, not a WIT change.

## What the walkthrough changed

Each of these came from a mapping that read badly in an earlier draft:

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
   cluttered both Plugins (SoundCloud has no playlist writes in legacy, no
   batch liked state). Optional interfaces removed the stubs and the
   capability flags.
5. **No `connect` export.** Legacy's `ConfigureConnection` validated
   credentials and fetched the account label on every start. The host now
   holds the account from sign-in and only calls `refresh-credential` when
   needed.
6. **Like is not an action.** The feed attached a Plugin "Like" action next to
   the host's like control. Only Provider-specific actions (Repost) remain.
7. **Liked state can be host-derived,** which removed SoundCloud's most
   intricate in-Plugin cache.

## Decisions and open questions for the user

Decisions taken in this draft that the user should confirm or overturn:

1. **Raw TCP (`socket`).** Only Spotify needs it, for audio keys from the
   access point. Without it Spotify cannot play at all under ADR 0004. It is
   manifest-declared per host and port and shown separately on the consent
   screen. Accept it, or accept that Spotify playback is out of scope?
2. **Web capture is declarative.** Legacy's `WEB_CAPTURE_FRAMEWORK.md`
   deliberately chose an interactive loop over a rules engine. This draft
   reverses that: the Plugin declares values, sources and page scripts, and
   the host decides completion. SoundCloud fits, and page scripts with
   `ristle.capture` are the escape hatch for values hidden in the DOM or in
   responses a script can fetch. What is lost is step logic that depends on
   values seen so far. Keep declarative, or add an optional
   `observe-capture` export later if a Provider needs it?
3. **Plugins are stateless between calls.** Any instance may serve any call;
   memory is a cache. This makes concurrency and recovery simple and is why
   the host keeps authorization state, credentials and liked-state indexes. A
   Plugin that needs a warm connection (Spotify's access point) reconnects
   after its instance is discarded. Confirm.
4. **My Library Search is host-side.** No Plugin searches its own library.
   Providers with a server-side library search (none of the two) cannot use
   it in 0.1.
5. **Provider Views are rows, shelves and grids** of standard items, with
   notes and actions per row (open question 1 in `requirements.md`). No
   hero, chart, station or article sections. Add them when a Plugin needs
   one?
6. **Partial write failure** (open question 2): writes are not atomic. The host
   chunks by manifest limits and reports per chunk; a Plugin reports refused
   items in `outcome.rejected`. Enough?
7. **No background tasks** (open question 3) in 0.1. Feeds refresh through the
   host's revalidation when opened. Notifications for new feed items would
   need a scheduled export with a budget; deferred.
8. **Media hosts may be wildcards** such as `*.scdn.co`. The host fetches
   stream URLs on the Plugin's behalf, so this is an exfiltration channel if
   it is too broad. Spotify's CDN list includes `audio-ak-spotify-com.akamaized.net`,
   which is exact; is `*.akamaized.net` ever acceptable?
9. **Device-code sign-in is kept** although neither Plugin uses it, because
   TV-style Providers need it and it costs one case.
10. **`gain-db` is in the Stream Source** for Providers that publish an
    intended level (Spotify). Ristle legacy did not normalize. Keep or drop?

Open questions:

11. **Versioning and deprecation** (open question 4). WIT records cannot gain
    fields compatibly, so any new `track` field is a new package version. The
    proposal: stay at `0.x` until the first third-party Plugin ships, then
    `1.0`. Additions go into new interfaces or a new minor version; the host
    implements every `1.x` world side by side and keeps the previous major for
    12 months after a new one. Is 12 months right?
12. **Party metadata** (open question 5). Proposal: every field of `track`,
    `album`, `profile` and `playlist` may be relayed to guests; nothing from
    `credential`, `stream-source` or `storage` ever is. Is any Track field
    too private, for example a private SoundCloud upload's `url`?
13. **Spotify feasibility.** The access point and spclient-over-HTTPS paths
    above are unverified. A short spike should confirm them before the WIT is
    final, because they decide whether `socket` stays.
14. **Queue Projection and settings pages** are not in 0.1. Both were reserved
    but unused in legacy. Confirm they can wait.
15. **SoundCloud playlist writes** were never enabled in legacy. Should the
    port add them (needs a measured web-session `PUT`), or keep parity?
