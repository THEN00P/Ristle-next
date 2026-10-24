//! A Provider Plugin with scripted behavior, for `ristle-plugin-host`'s
//! conformance tests. It implements every export of `full-provider`.
//!
//! `catalog.tracks` reads its first id as a script:
//!
//! | First id | Behavior |
//! |---|---|
//! | `trap` | traps (`unreachable`) |
//! | `loop` | runs forever |
//! | `spin:<ms>` | runs guest code for `<ms>` milliseconds of wall time |
//! | `grow:<mib>` | allocates and keeps `<mib>` MiB of linear memory |
//! | `count` | how many calls this instance has served, as a title |
//! | `system` | `system.now` and `system.locale` as titles |
//! | `log:<n>` | logs `<n>` messages |
//! | `crypto` | the hex SHA-256 of `abc` as a title |
//! | `random`, `too-random` | 16 random bytes as hex; asks for 4096 |
//! | `store:<key>=<value>`, `store:<key>` | `storage.set`; `storage.get` as a title |
//! | `fill:<n>` | stores `<n>` values of 1 MiB |
//! | `huge-title`, `huge-error`, `huge-item-error` | a title, a call's error or an item's error over 64 KiB |
//! | `errors:<n>` | `<n>` unavailable Tracks with 60 KiB reasons |
//! | `many:<n>` | `<n>` Tracks with 60 KiB titles and album names |
//! | `rich` | Tracks with every field set, some of them invalid |
//! | `account`, `server`, `folders`, `id`, `credential` | what `connection` reports, as a title |
//! | `set-credential:<scheme>[:<host>:<authorization>]` | stores a credential, attaching `Authorization` to `<host>` |
//! | `data:<key>=<value>`, `data:<key>` | `connection.set-data`; `get-data` as a title |
//! | `fetch:<url>`, `fetch-bypass:<url>` | one GET, cached by HTTP rules or not, described as a title |
//! | `fetch-all:<n>:<url>` | `<n>` GETs at once, one title per outcome |
//! | `slow-write:<scheme>:<url>` | reads the credential, fetches `<url>`, then writes |
//! | anything else | a Track per id; ids starting `gone` are unavailable |
//!
//! Every other export returns fixed, predictable values.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

wit_bindgen::generate!({ path: "../../wit", world: "full-provider" });

use exports::ristle::plugin::{
    accounts, byte_source, catalog, comments, feed, like_lookup, likes, playlist_editing,
    queue_projection, reposts, settings,
};
use ristle::plugin::connection::Connection;
use ristle::plugin::types::{
    Account, Addressing, Collection, Credential, Entry, Error, Item, ItemRef, Library, LibraryKind,
    ListRequest, Listing, Location, Named, Page, Playlist, PluginBytes, SettingValue, Shelf,
    StreamSource, Target, Track, TrackTraits, ViewPage,
};
use ristle::plugin::{crypto, http, storage, system};

struct Mock;

export!(Mock);

/// Calls served by this instance. Linear memory is the only place it lives,
/// so a fresh instance starts at zero.
static CALLS: AtomicU64 = AtomicU64::new(0);

/// Memory kept alive by `grow:`.
static KEPT: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

fn track(id: &str, title: &str) -> Track {
    Track {
        id: id.to_string(),
        title: title.to_string(),
        artists: vec![Named {
            id: Some("artist-1".to_string()),
            name: "Mock Artist".to_string(),
        }],
        album: None,
        duration_ms: Some(180_000),
        artwork: Vec::new(),
        traits: TrackTraits::empty(),
        released: None,
        uploaded: None,
        unavailable: None,
        isrc: None,
        url: None,
        attributes: Vec::new(),
        file: None,
    }
}

fn titles(titles: Vec<String>) -> catalog::SearchResults {
    catalog::SearchResults {
        groups: vec![catalog::SearchGroup {
            kind: catalog::SearchKind::Tracks,
            items: titles
                .iter()
                .enumerate()
                .map(|(index, title)| Item::Track(track(&format!("t{index}"), title)))
                .collect(),
            next: None,
        }],
    }
}

type Tracks = Result<Vec<Result<Track, Error>>, Error>;

fn titled(titles: Vec<String>) -> Tracks {
    Ok(titles
        .iter()
        .enumerate()
        .map(|(index, title)| Ok(track(&format!("t{index}"), title)))
        .collect())
}

/// Runs a script command given as the first id of `catalog.tracks`.
fn script(conn: &Connection, command: &str, calls: u64) -> Option<Tracks> {
    if let Some(result) = connection_script(conn, command) {
        return Some(result);
    }
    if command == "trap" {
        core::arch::wasm32::unreachable();
    }
    if command == "loop" {
        let mut counter = 0u64;
        loop {
            counter = core::hint::black_box(counter.wrapping_add(1));
        }
    }
    if let Some(ms) = command.strip_prefix("spin:") {
        spin(ms.parse().unwrap_or(0));
        return Some(titled(vec!["spun".to_string()]));
    }
    if let Some(mib) = command.strip_prefix("grow:") {
        grow(mib.parse().unwrap_or(0));
        return Some(titled(vec!["grown".to_string()]));
    }
    if let Some(count) = command.strip_prefix("log:") {
        for index in 0..count.parse::<u32>().unwrap_or(0) {
            system::log(system::Level::Info, &format!("message {index}"));
        }
        return Some(titled(vec!["logged".to_string()]));
    }
    if let Some(key) = command.strip_prefix("store:") {
        // `store:<key>=<value>` sets, `store:<key>` reads.
        return Some(match key.split_once('=') {
            Some((key, value)) => match storage::set(key, value.as_bytes(), None) {
                Ok(()) => titled(vec!["stored".to_string()]),
                Err(error) => Err(error),
            },
            None => titled(vec![
                storage::get(key)
                    .map(|value| String::from_utf8_lossy(&value).into_owned())
                    .unwrap_or_else(|| "none".to_string()),
            ]),
        });
    }
    if let Some(megabytes) = command.strip_prefix("fill:") {
        // Stores `<n>` values of 1 MiB under fill-0, fill-1, …
        let value = vec![7u8; 1024 * 1024];
        for index in 0..megabytes.parse::<u32>().unwrap_or(0) {
            if let Err(error) = storage::set(&format!("fill-{index}"), &value, None) {
                return Some(Err(error));
            }
        }
        return Some(titled(vec!["filled".to_string()]));
    }
    match command {
        "count" => Some(titled(vec![calls.to_string()])),
        "system" => Some(titled(vec![system::now().to_string(), system::locale()])),
        "crypto" => Some(titled(vec![hex(&crypto::digest(
            crypto::Hash::Sha256,
            b"abc",
        ))])),
        "random" => Some(match crypto::random(16) {
            Ok(bytes) => titled(vec![hex(&bytes)]),
            Err(error) => Err(error),
        }),
        "too-random" => Some(crypto::random(4096).map(|_| Vec::new())),
        "huge-title" => Some(titled(vec!["x".repeat(64 * 1024 + 1)])),
        "huge-error" => Some(Err(Error::Transient("e".repeat(64 * 1024 + 1)))),
        "huge-item-error" => Some(Ok(vec![Err(Error::Unavailable("e".repeat(64 * 1024 + 1)))])),
        command if command.starts_with("errors:") => {
            // `<n>` unavailable Tracks whose reasons are 60 KiB each.
            let count: usize = command["errors:".len()..].parse().unwrap_or(0);
            Some(Ok((0..count)
                .map(|_| Err(Error::Unavailable("r".repeat(60 * 1024))))
                .collect()))
        }
        command if command.starts_with("many:") => {
            let count: usize = command["many:".len()..].parse().unwrap_or(0);
            let name = "y".repeat(60 * 1024);
            Some(Ok((0..count)
                .map(|index| {
                    let mut big = track(&format!("t{index}"), &name);
                    big.album = Some(Named {
                        id: None,
                        name: name.clone(),
                    });
                    Ok(big)
                })
                .collect()))
        }
        "fetch" => Some(Err(Error::Internal(
            "fetch arrives with the connection services".to_string(),
        ))),
        "rich" => {
            let mut rich = track("rich", "Rich");
            rich.url = Some("https://example.com/track/rich".to_string());
            rich.artwork = vec![
                ristle::plugin::types::Image {
                    url: "https://i.mock.example/a.jpg".to_string(),
                    width: Some(500),
                    height: Some(500),
                },
                ristle::plugin::types::Image {
                    url: "http://i.mock.example/insecure.jpg".to_string(),
                    width: None,
                    height: None,
                },
                ristle::plugin::types::Image {
                    url: "https://elsewhere.example/b.jpg".to_string(),
                    width: None,
                    height: None,
                },
            ];
            rich.released = Some(ristle::plugin::types::MediaDate {
                year: 2024,
                month: Some(2),
                day: Some(30),
            });
            rich.uploaded = Some(ristle::plugin::types::MediaDate {
                year: 2024,
                month: Some(5),
                day: None,
            });
            rich.traits = TrackTraits::EXPLICIT | TrackTraits::UNSEEKABLE;
            rich.attributes = vec![
                ristle::plugin::types::Attribute {
                    key: "genre".to_string(),
                    value: ristle::plugin::types::AttributeValue::Text("House".to_string()),
                },
                ristle::plugin::types::Attribute {
                    key: "plays".to_string(),
                    value: ristle::plugin::types::AttributeValue::Number(12.0),
                },
            ];
            let mut link = track("link", "Link");
            link.url = Some("javascript:alert(1)".to_string());
            link.unavailable = Some("region".to_string());
            Some(Ok(vec![Ok(rich), Ok(link)]))
        }
        _ => None,
    }
}

fn get(url: &str, cache: http::Cache) -> http::Request {
    http::Request {
        method: http::Method::Get,
        url: url.to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        cache,
        timeout_ms: None,
    }
}

/// A response as a title: its body as text when short, else its length.
fn describe(response: &http::Response) -> String {
    let body = if response.body.len() <= 64 {
        String::from_utf8_lossy(&response.body).into_owned()
    } else {
        format!("<{} bytes>", response.body.len())
    };
    format!(
        "{} {} cached={} sequence={}",
        response.status, body, response.cached, response.sequence
    )
}

/// Scripts for the `connection` resource.
fn connection_script(conn: &Connection, command: &str) -> Option<Tracks> {
    let (name, argument) = command.split_once(':').unwrap_or((command, ""));
    let result = match name {
        "account" => titled(vec![
            conn.account()
                .map_or("none".to_string(), |account| account.name),
        ]),
        "server" => titled(vec![
            conn.server()
                .map_or("none".to_string(), |server| server.name),
        ]),
        "folders" => titled(vec![conn.folders().len().to_string()]),
        "id" => titled(vec![conn.id()]),
        "credential" => titled(vec![
            conn.credential()
                .map_or("none".to_string(), |credential| credential.scheme),
        ]),
        // `set-credential:<scheme>` or `set-credential:<scheme>:<host>:<authorization>`
        "set-credential" => {
            let mut parts = argument.splitn(3, ':');
            let scheme = parts.next().unwrap_or_default().to_string();
            let attach = match (parts.next(), parts.next()) {
                (Some(host), Some(value)) => vec![ristle::plugin::types::Attach {
                    hosts: vec![host.to_string()],
                    headers: vec![("Authorization".to_string(), value.to_string())],
                    query: Vec::new(),
                }],
                _ => Vec::new(),
            };
            conn.set_credential(&credential(scheme, attach))
                .map(|()| vec![Ok(track("t0", "set"))])
        }
        // `data:<key>=<value>` sets, `data:<key>` reads.
        "data" => match argument.split_once('=') {
            Some((key, value)) => conn
                .set_data(key, value.as_bytes(), None)
                .map(|()| vec![Ok(track("t0", "stored"))]),
            None => titled(vec![
                conn.get_data(argument)
                    .map(|value| String::from_utf8_lossy(&value).into_owned())
                    .unwrap_or_else(|| "none".to_string()),
            ]),
        },
        "fetch" => conn
            .fetch(&get(argument, http::Cache::Http))
            .map(|response| vec![Ok(track("t0", &describe(&response)))]),
        "fetch-bypass" => conn
            .fetch(&get(argument, http::Cache::Bypass))
            .map(|response| vec![Ok(track("t0", &describe(&response)))]),
        // `fetch-all:<n>:<url>`: one Track per request, titled by its outcome.
        "fetch-all" => {
            let (count, url) = argument.split_once(':').unwrap_or(("0", ""));
            let count: usize = count.parse().unwrap_or(0);
            let requests = vec![get(url, http::Cache::Bypass); count];
            conn.fetch_all(&requests).map(|results| {
                results
                    .iter()
                    .enumerate()
                    .map(|(index, result)| {
                        let title = match result {
                            Ok(response) => describe(response),
                            Err(error) => format!("error {error:?}"),
                        };
                        Ok(track(&format!("t{index}"), &title))
                    })
                    .collect()
            })
        }
        // `slow-write:<scheme>:<url>`: reads the credential, fetches a slow
        // URL, then writes, so another call can write in between.
        "slow-write" => {
            let (scheme, url) = argument.split_once(':').unwrap_or((argument, ""));
            let _ = conn.credential();
            match conn.fetch(&get(url, http::Cache::Bypass)) {
                Ok(_) => conn
                    .set_credential(&credential(scheme.to_string(), Vec::new()))
                    .map(|()| vec![Ok(track("t0", "set"))]),
                Err(error) => Err(error),
            }
        }
        _ => return None,
    };
    Some(result)
}

fn credential(scheme: String, attach: Vec<ristle::plugin::types::Attach>) -> Credential {
    Credential {
        scheme,
        access_token: None,
        refresh_token: None,
        expires_at: None,
        scopes: Vec::new(),
        username: None,
        password: None,
        custom: None,
        attach,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn spin(ms: u64) {
    let until = system::now() + ms;
    while system::now() < until {
        core::hint::spin_loop();
    }
}

fn grow(mib: usize) {
    let mut block = vec![0u8; mib * 1024 * 1024];
    // Touch every page so the memory is really used.
    for index in (0..block.len()).step_by(4096) {
        block[index] = 1;
    }
    if let Ok(mut kept) = KEPT.lock() {
        kept.push(block);
    }
}

impl catalog::Guest for Mock {
    fn search(
        _conn: &Connection,
        request: catalog::SearchRequest,
    ) -> Result<catalog::SearchResults, Error> {
        CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(titles(vec![request.query]))
    }

    fn my_library(_conn: &Connection) -> Result<Library, Error> {
        Ok(Library {
            shelves: vec![Shelf {
                kind: LibraryKind::LikedTracks,
                title: "Liked".to_string(),
                collection: Collection {
                    id: "liked".to_string(),
                    addressing: Addressing::Offset,
                    total: None,
                    revision: None,
                    endless: false,
                },
            }],
            views: Vec::new(),
        })
    }

    fn open(_conn: &Connection, target: Target) -> Result<Page, Error> {
        match target {
            Target::View(view) => Ok(Page::View(ViewPage {
                title: view.view,
                subtitle: None,
                artwork: Vec::new(),
                sections: Vec::new(),
            })),
            _ => Err(Error::Unavailable("the mock has only views".to_string())),
        }
    }

    fn list(
        _conn: &Connection,
        collection: String,
        request: ListRequest,
    ) -> Result<Listing, Error> {
        let entries = (0..request.limit.min(10))
            .map(|index| Entry {
                item: Item::TrackId(format!("{collection}-{index}")),
                added: None,
                added_by: None,
            })
            .collect();
        Ok(Listing {
            entries,
            next: None,
            total: None,
            revision: None,
        })
    }

    fn tracks(_conn: &Connection, ids: Vec<String>) -> Result<Vec<Result<Track, Error>>, Error> {
        let calls = CALLS.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(first) = ids.first()
            && let Some(result) = script(_conn, first, calls)
        {
            return result;
        }
        Ok(ids
            .iter()
            .map(|id| {
                if id.starts_with("gone") {
                    Err(Error::Unavailable("removed".to_string()))
                } else {
                    Ok(track(id, &format!("Track {id}")))
                }
            })
            .collect())
    }

    fn resolve(
        _conn: &Connection,
        track: String,
        _quality: catalog::Quality,
    ) -> Result<StreamSource, Error> {
        Ok(StreamSource {
            location: Location::Bytes(PluginBytes {
                handle: track,
                length: Some(1024),
            }),
            mime: Some("audio/ogg".to_string()),
            duration_ms: Some(180_000),
            expires_at: None,
        })
    }
}

impl accounts::Guest for Mock {
    fn begin_authorization(
        _conn: &Connection,
        _context: accounts::SessionContext,
    ) -> Result<accounts::Step, Error> {
        Ok(accounts::Step::Ask(accounts::Ask {
            challenge: accounts::Challenge::Form(accounts::Form {
                title: "Sign in".to_string(),
                instructions: None,
                fields: vec![accounts::Field {
                    name: "token".to_string(),
                    label: "Token".to_string(),
                    kind: accounts::FieldKind::Secret,
                    required: true,
                    placeholder: None,
                    value: None,
                }],
                help_url: None,
            }),
            state: "form".to_string(),
        }))
    }

    fn continue_authorization(
        _conn: &Connection,
        _context: accounts::SessionContext,
        _state: String,
        answer: accounts::Answer,
    ) -> Result<accounts::Step, Error> {
        match answer {
            accounts::Answer::Form(fields) => {
                let token = fields
                    .into_iter()
                    .find(|(name, _)| name == "token")
                    .map(|(_, value)| value)
                    .unwrap_or_default();
                Ok(accounts::Step::Done(accounts::Authorized {
                    account: Account {
                        id: format!("account-{token}"),
                        name: "Mock User".to_string(),
                        avatar: Vec::new(),
                    },
                }))
            }
            _ => Err(Error::Internal("unexpected answer".to_string())),
        }
    }

    fn refresh_credential(_conn: &Connection) -> Result<Credential, Error> {
        Err(Error::Unauthorized("the mock cannot refresh".to_string()))
    }
}

impl likes::Guest for Mock {
    fn set_liked(_conn: &Connection, _item: ItemRef, _liked: bool) -> Result<(), Error> {
        Ok(())
    }
}

impl like_lookup::Guest for Mock {
    fn liked_states(_conn: &Connection, items: Vec<ItemRef>) -> Result<Vec<bool>, Error> {
        Ok(items
            .iter()
            .map(|item| matches!(item, ItemRef::Track(id) if id.starts_with("liked")))
            .collect())
    }
}

impl playlist_editing::Guest for Mock {
    fn create(
        _conn: &Connection,
        playlist: playlist_editing::NewPlaylist,
    ) -> Result<Playlist, Error> {
        Ok(Playlist {
            id: "new-playlist".to_string(),
            title: playlist.title,
            description: playlist.description,
            owner: None,
            artwork: Vec::new(),
            track_count: Some(0),
            created: None,
            updated: None,
            public: Some(playlist.public),
            editable: true,
            folder: None,
            url: None,
        })
    }

    fn edit(
        _conn: &Connection,
        _playlist: String,
        revision: Option<String>,
        _change: playlist_editing::Change,
    ) -> Result<playlist_editing::Outcome, Error> {
        Ok(playlist_editing::Outcome {
            revision: revision.map(|revision| format!("{revision}+1")),
            rejected: Vec::new(),
        })
    }

    fn delete(_conn: &Connection, _playlist: String) -> Result<(), Error> {
        Ok(())
    }
}

impl reposts::Guest for Mock {
    fn set_reposted(_conn: &Connection, _item: ItemRef, _reposted: bool) -> Result<(), Error> {
        Ok(())
    }
}

impl feed::Guest for Mock {
    fn posts(
        _conn: &Connection,
        _cursor: Option<String>,
        _limit: u32,
    ) -> Result<feed::FeedPage, Error> {
        Ok(feed::FeedPage {
            posts: Vec::new(),
            next: None,
        })
    }
}

impl comments::Guest for Mock {
    fn comments(
        _conn: &Connection,
        _item: ItemRef,
        _cursor: Option<String>,
        _limit: u32,
    ) -> Result<comments::CommentPage, Error> {
        Ok(comments::CommentPage {
            comments: Vec::new(),
            next: None,
        })
    }
}

impl settings::Guest for Mock {
    fn describe(_conn: &Connection) -> Result<Vec<settings::Group>, Error> {
        Ok(vec![settings::Group {
            title: None,
            settings: vec![settings::Setting {
                key: "reposts".to_string(),
                title: "Show reposts".to_string(),
                description: None,
                scope: settings::Scope::Connection,
                control: settings::Control::Toggle(true),
            }],
        }])
    }

    fn apply(
        _conn: &Connection,
        key: String,
        _value: SettingValue,
    ) -> Result<Option<String>, Error> {
        Ok(Some(format!("{key} saved")))
    }
}

impl queue_projection::Guest for Mock {
    fn start(_conn: &Connection, _state: queue_projection::QueueState) -> Result<(), Error> {
        Ok(())
    }

    fn update(_conn: &Connection, _state: queue_projection::QueueState) -> Result<(), Error> {
        Ok(())
    }

    fn receive(
        _conn: &Connection,
        _channel: String,
        _frame: ristle::plugin::live::Frame,
        state: queue_projection::QueueState,
    ) -> Result<queue_projection::Mutations, Error> {
        Ok(queue_projection::Mutations {
            revision: state.revision,
            changes: vec![queue_projection::Mutation::Pause],
        })
    }

    fn stop(_conn: &Connection) -> Result<(), Error> {
        Ok(())
    }
}

impl byte_source::Guest for Mock {
    fn read(
        _conn: &Connection,
        handle: String,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, Error> {
        // Byte `i` of every stream is `i mod 251`, so reads are checkable.
        let end = (offset + u64::from(length)).min(1024);
        let _ = handle;
        Ok((offset..end.max(offset))
            .map(|index| (index % 251) as u8)
            .collect())
    }
}
