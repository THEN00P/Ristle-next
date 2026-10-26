//! The committed schema-v1 fixture and the tests that run against it.
//!
//! `tests/fixtures/schema-v1.sql` is a text dump of a `ristle.sqlite` that
//! schema v1 wrote, with rows in every table. It was generated once by
//! `write_schema_v1_fixture` (run with `--ignored`) and must never be
//! regenerated: it stands for the databases users have. Tests check that
//!
//! - the current build reads every row of it back exactly, so a change to
//!   the serde shape of a stored domain type cannot break existing data
//!   unnoticed;
//! - a fresh database has exactly its schema, so schema v1 cannot be edited
//!   in place; changes go through a migration;
//! - the migration runner upgrades it, and rolls a failing migration back
//!   leaving it untouched. Every future migration adds a test here that
//!   applies it to the fixture.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use ristle_domain::{
    Addressing, Artwork, AttributeValue, Availability, CollectionDescriptor, CollectionRef, Count,
    ManualClock, MediaDate, MetadataAttribute, Named, PlaybackCapabilities, PlaylistId,
    RistlePlaylist, Track, TrackRef,
};
use ristle_queue::sequence::{
    Opened, SequenceDescriptor, SequenceId, SequenceSource, Slot, SlotId,
};
use ristle_queue::{
    CollectionPosition, Current, EntryId, Origin, QueueEntry, QueueState, RepeatMode,
    SequenceState, Submitter,
};
use rusqlite::Connection;
use rusqlite::types::Value;

use crate::repo::{
    Account, ConfirmedOrigin, ConnectionRecord, FolderGrant, Intent, KvScope, LocalContent,
    LocalFile, PlaybackSession, PluginRecord, ServerGrant,
};
use crate::{OpenStatus, Storage, StoragePaths};

const FIXTURE: &str = include_str!("../tests/fixtures/schema-v1.sql");
const NOW: u64 = 1_760_000_000_000;

fn track(provider: &str, id: &str) -> Track {
    Track {
        artists: vec![
            Named {
                id: Some("a1".into()),
                name: "Artist One".into(),
            },
            Named {
                id: None,
                name: "Guest".into(),
            },
        ],
        album: Some(Named {
            id: Some("al1".into()),
            name: "Album".into(),
        }),
        duration_ms: Some(215_000),
        artwork: vec![Artwork {
            url: "https://img.example/1.jpg".into(),
            width: Some(300),
            height: None,
        }],
        released: Some(MediaDate::new(1999, Some(12), Some(31)).unwrap()),
        uploaded: Some(MediaDate::new(2001, None, None).unwrap()),
        explicit: true,
        capabilities: PlaybackCapabilities {
            seekable: false,
            live: false,
        },
        availability: Availability::Available,
        url: Some(format!("https://{provider}.example/track/{id}")),
        isrc: Some("GBAYE0000001".into()),
        attributes: vec![
            MetadataAttribute {
                key: "genre".into(),
                value: AttributeValue::Text("ambient".into()),
            },
            MetadataAttribute {
                key: "bpm".into(),
                value: AttributeValue::Number(92.5),
            },
            MetadataAttribute {
                key: "soundcloud:reposted".into(),
                value: AttributeValue::YesNo(false),
            },
        ],
        ..Track::new(
            TrackRef::catalog(provider.into(), id.into()).unwrap(),
            format!("Title {id}"),
        )
    }
}

fn server_track() -> Track {
    Track {
        availability: Availability::Unavailable {
            reason: Some("Server unreachable".into()),
        },
        ..Track::new(
            TrackRef::server_scoped("jellyfin".into(), "item-9".into(), "jellyfin-1".into())
                .unwrap(),
            "On my server",
        )
    }
}

/// Everything the fixture holds, as values.
struct Contents {
    session: PlaybackSession,
    sequence: SequenceState,
    playlist: RistlePlaylist,
    recent: Vec<Track>,
    settings: Vec<(&'static str, &'static str)>,
    plugin: PluginRecord,
    account: Account,
    permissions: Vec<String>,
    folders: Vec<FolderGrant>,
    server: ServerGrant,
    local_content: LocalContent,
    local_track: Track,
    local_files: Vec<LocalFile>,
}

fn contents() -> Contents {
    let entry =
        |id: u64, track: Track, origin: Origin, slot: Option<u64>, submitter: Option<Submitter>| {
            QueueEntry {
                id: EntryId(id),
                track,
                origin,
                slot: slot.map(SlotId),
                submitter,
            }
        };
    let at = |position: u64| Origin::Collection {
        position: CollectionPosition(position),
    };
    let liked = CollectionDescriptor {
        reference: CollectionRef {
            connection: "spotify-1".into(),
            id: "liked".into(),
        },
        addressing: Addressing::Offset,
        total: Some(Count::Estimate(4)),
        revision: Some("rev-1".into()),
        endless: false,
    };
    let feed = CollectionDescriptor {
        reference: CollectionRef {
            connection: "soundcloud-1".into(),
            id: "stream".into(),
        },
        addressing: Addressing::Cursor,
        total: None,
        revision: None,
        endless: true,
    };
    Contents {
        session: PlaybackSession {
            queue: QueueState {
                collection: vec![
                    entry(1, track("spotify", "s1"), at(0), Some(0), None),
                    entry(
                        5,
                        track("soundcloud", "passed"),
                        Origin::Request,
                        None,
                        None,
                    ),
                    entry(2, track("spotify", "s1"), at(3), Some(1), None),
                    entry(3, server_track(), at(4), None, None),
                ],
                cursor: Some(2),
                requests: vec![
                    entry(7, track("spotify", "next"), Origin::Request, None, None),
                    entry(
                        8,
                        track("soundcloud", "later"),
                        Origin::Request,
                        None,
                        Some(Submitter {
                            id: "member-1".into(),
                            display_name: "Ada".into(),
                        }),
                    ),
                ],
                play_next_count: 1,
                current: Some(Current::Detached(Box::new(entry(
                    6,
                    track("soundcloud", "playing"),
                    Origin::Request,
                    None,
                    None,
                )))),
                repeat: RepeatMode::Queue,
                shuffle: Some(u64::MAX - 1),
                next_id: EntryId(9),
            },
            sequence: Some(SequenceId("liked:spotify-1+soundcloud-1".into())),
            position_ms: 61_000,
            intent: Intent::Playing,
        },
        sequence: SequenceState {
            descriptor: SequenceDescriptor {
                id: SequenceId("liked:spotify-1+soundcloud-1".into()),
                title: "Liked Songs".into(),
                opened: Opened::LikedTracks,
                sources: vec![
                    SequenceSource::Collection(liked),
                    SequenceSource::Collection(feed),
                    SequenceSource::RistlePlaylist(PlaylistId::new("road")),
                    SequenceSource::Fixed,
                ],
            },
            slots: vec![
                Slot {
                    id: SlotId(0),
                    source: 0,
                    offset: 0,
                    track: Some(TrackRef::catalog("spotify".into(), "s1".into()).unwrap()),
                },
                Slot {
                    id: SlotId(1),
                    source: 0,
                    offset: 3,
                    track: Some(TrackRef::catalog("spotify".into(), "s1".into()).unwrap()),
                },
                Slot {
                    id: SlotId(2),
                    source: 0,
                    offset: 1,
                    track: None,
                },
                Slot {
                    id: SlotId(4),
                    source: 1,
                    offset: 0,
                    track: Some(TrackRef::catalog("soundcloud".into(), "x".into()).unwrap()),
                },
                Slot {
                    id: SlotId(3),
                    source: 3,
                    offset: 0,
                    track: Some(server_track().reference),
                },
            ],
            order: vec![SlotId(3), SlotId(0), SlotId(1), SlotId(4), SlotId(2)],
            cursor: Some(2),
            materialized_end: 4,
            shuffle: Some(1 << 63),
            revision: 3,
            next_slot: SlotId(5),
        },
        playlist: RistlePlaylist {
            id: PlaylistId::new("road"),
            title: "Road trip".into(),
            tracks: vec![
                track("spotify", "s1"),
                Track {
                    availability: Availability::Unavailable { reason: None },
                    ..track("soundcloud", "gone")
                },
                track("spotify", "s1"),
                server_track(),
            ],
            created_ms: NOW - 86_400_000,
            updated_ms: NOW - 3_600_000,
        },
        recent: vec![track("spotify", "s1"), track("soundcloud", "playing")],
        settings: vec![("audio.volume", "0.8"), ("ui.theme", "dark")],
        plugin: PluginRecord {
            id: "spotify".into(),
            version: "0.3.1".into(),
            package_hash: "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                .into(),
            publisher_key: "ed25519:3fa1".into(),
            enabled: true,
            installed_ms: NOW - 1_000,
            updated_ms: NOW - 500,
        },
        account: Account {
            id: "user-1".into(),
            name: "Ada L.".into(),
            avatar: vec![Artwork {
                url: "https://img.example/a.jpg".into(),
                width: Some(64),
                height: Some(64),
            }],
        },
        permissions: vec!["credentials".into(), "hosts:api.spotify.com".into()],
        folders: vec![FolderGrant {
            id: "f1".into(),
            name: "Music".into(),
            locator: "/home/ada/Music".into(),
        }],
        server: ServerGrant {
            name: "Living Room".into(),
            urls: vec![
                "http://192.168.1.20:8096".into(),
                "https://music.example/jellyfin".into(),
            ],
            confirmed: vec![
                ConfirmedOrigin {
                    origin: "http://192.168.1.20:8096".into(),
                    public: false,
                    addresses: vec!["192.168.1.20".into()],
                },
                ConfirmedOrigin {
                    origin: "https://music.example".into(),
                    public: true,
                    addresses: vec![],
                },
            ],
        },
        local_content: LocalContent {
            content_id: "content-1".into(),
            fingerprint: "AQAAE0mUaEkSRZEGAA".into(),
            fingerprint_hash: 0xfeed_beef,
            duration_ms: Some(180_000),
        },
        local_track: Track {
            availability: Availability::Unknown,
            ..track("local-files", "content-1")
        },
        local_files: vec![
            LocalFile {
                location: "f1/a.flac".into(),
                content_id: "content-1".into(),
                format: "flac".into(),
                bitrate_kbps: 900,
                size_bytes: 20_000_000,
                modified_ms: NOW - 9,
                available: true,
            },
            LocalFile {
                location: "f1/a.mp3".into(),
                content_id: "content-1".into(),
                format: "mp3".into(),
                bitrate_kbps: 320,
                size_bytes: 7_000_000,
                modified_ms: NOW - 8,
                available: false,
            },
        ],
    }
}

fn populate(storage: &Storage, clock: &ManualClock) {
    let c = contents();
    let (plugin, account, permissions, folders, server) = (
        c.plugin.clone(),
        c.account.clone(),
        c.permissions.clone(),
        c.folders.clone(),
        c.server.clone(),
    );
    storage
        .write_blocking(move |tx| {
            tx.plugins().put(&plugin)?;
            tx.plugins().put(&PluginRecord {
                id: "local-files".into(),
                ..plugin.clone()
            })?;
            tx.connections()
                .create(&"spotify-1".into(), &"spotify".into())?;
            tx.connections()
                .create(&"local-1".into(), &"local-files".into())?;
            tx.connections()
                .begin_generation(&"spotify-1".into(), Some(&account))?;
            tx.grants()
                .set_permissions(&"spotify".into(), &permissions)?;
            tx.grants().set_folders(&"local-1".into(), &folders)?;
            tx.grants().set_server(&"spotify-1".into(), Some(&server))?;
            tx.plugin_kv().set(
                &KvScope::Plugin("spotify".into()),
                "client-id",
                b"abc",
                None,
                1 << 20,
            )?;
            tx.plugin_kv().set(
                &KvScope::Connection("spotify-1".into()),
                "user",
                &[0, 159, 255],
                Some(3600),
                1 << 20,
            )?;
            tx.connections().reserve_response_sequences(
                &"spotify".into(),
                &"spotify-1".into(),
                1000,
            )?;
            tx.connections()
                .reserve_response_sequences(&"spotify".into(), &"".into(), 10)?;
            tx.receipts().put("party-1", "member-1:1", "applied")?;
            tx.receipts()
                .put("party-1", "member-2:1", "rejected: unavailable")?;
            for (key, value) in [("audio.volume", "0.8"), ("ui.theme", "dark")] {
                tx.settings().put(key, value)?;
            }
            Ok(())
        })
        .unwrap();
    let (content, local_track, files) = (
        c.local_content.clone(),
        c.local_track.clone(),
        c.local_files.clone(),
    );
    storage
        .write_blocking(move |tx| {
            for file in &files {
                tx.local_index()
                    .upsert(&"local-1".into(), &content, &local_track, file)?;
            }
            Ok(())
        })
        .unwrap();
    let (session, sequence, playlist) = (c.session.clone(), c.sequence.clone(), c.playlist.clone());
    storage
        .write_blocking(move |tx| {
            tx.playlists().save(&playlist)?;
            tx.sequences().save(&sequence)?;
            tx.session().save(&session)?;
            Ok(())
        })
        .unwrap();
    for track in c.recent {
        clock.advance(1_000);
        storage
            .write_blocking(move |tx| tx.recently_played().record(&track, 120_000))
            .unwrap();
    }
}

/// A logical dump: the schema, then every row, then the version.
fn dump(conn: &Connection) -> String {
    let mut out = String::from(
        "-- ristle.sqlite at schema v1, written by `write_schema_v1_fixture`.\n\
         -- Never regenerate it: it stands for the databases users have.\n",
    );
    let mut objects = conn
        // Tables, then indexes, each by name: an order that VACUUM and a
        // reload both keep.
        .prepare("SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type DESC, name")
        .unwrap();
    let objects: Vec<(String, String, String)> = objects
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for (_, _, sql) in &objects {
        writeln!(out, "{sql};").unwrap();
    }
    for (kind, table, _) in &objects {
        if kind != "table" {
            continue;
        }
        let select = conn.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let columns = select.column_count();
        let order: Vec<String> = (1..=columns).map(|i| i.to_string()).collect();
        drop(select);
        let mut select = conn
            .prepare(&format!(
                "SELECT * FROM {table} ORDER BY {}",
                order.join(", ")
            ))
            .unwrap();
        let mut rows = select.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let values: Vec<String> = (0..columns)
                .map(|i| literal(row.get::<_, Value>(i).unwrap()))
                .collect();
            writeln!(out, "INSERT INTO {table} VALUES ({});", values.join(", ")).unwrap();
        }
    }
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    writeln!(out, "PRAGMA user_version = {version};").unwrap();
    out
}

/// Asserts two dumps are equal, naming the first line that differs.
fn assert_same_dump(actual: &str, expected: &str) {
    let mismatch = actual
        .lines()
        .zip(expected.lines())
        .position(|(a, e)| a != e);
    if let Some(line) = mismatch {
        panic!(
            "dumps differ at line {}:
  {}
  {}",
            line + 1,
            actual.lines().nth(line).unwrap_or(""),
            expected.lines().nth(line).unwrap_or("")
        );
    }
    assert_eq!(
        actual.lines().count(),
        expected.lines().count(),
        "dumps differ in length"
    );
}

fn literal(value: Value) -> String {
    match value {
        Value::Null => "NULL".into(),
        Value::Integer(n) => n.to_string(),
        Value::Real(x) => format!("{x:?}"),
        Value::Text(s) => format!("'{}'", s.replace('\'', "''")),
        Value::Blob(b) => format!(
            "X'{}'",
            b.iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        ),
    }
}

/// A new database at `dir` loaded from the fixture.
fn load_fixture(dir: &Path) -> StoragePaths {
    let paths = StoragePaths::new(dir.join("data"), dir.join("cache"));
    std::fs::create_dir_all(&paths.data_dir).unwrap();
    // Rows are dumped table by table in name order, so foreign keys are
    // checked only once everything is in.
    let conn = Connection::open(paths.database()).unwrap();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute_batch(FIXTURE).unwrap();
    assert!(
        !conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
    paths
}

fn schema_of(path: &Path) -> Vec<(String, String, Option<String>)> {
    let conn = Connection::open(path).unwrap();
    let mut select = conn
        .prepare("SELECT type, name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name")
        .unwrap();
    select
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
#[ignore = "writes tests/fixtures/schema-v1.sql, which is committed and never regenerated"]
fn write_schema_v1_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let paths = StoragePaths::new(dir.path().join("data"), dir.path().join("cache"));
    let clock = Arc::new(ManualClock::new(NOW));
    let storage = Storage::open(&paths, clock.clone()).unwrap();
    populate(&storage, &clock);
    drop(storage);
    let text = dump(&Connection::open(paths.database()).unwrap());
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema-v1.sql");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(target, text).unwrap();
}

#[test]
fn schema_v1_fixture_reads_back_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let paths = load_fixture(dir.path());
    let clock = Arc::new(ManualClock::new(NOW));
    let storage = Storage::open(&paths, clock).unwrap();
    assert_eq!(storage.status(), &OpenStatus::Ready);
    assert!(
        !crate::migrate::sibling(&paths.database(), ".v1.backup").exists(),
        "v1 was migrated"
    );
    let c = contents();

    let read = storage
        .read_blocking(|tx| {
            Ok((
                tx.session().load_for_cold_start()?,
                tx.sequences()
                    .load(&SequenceId("liked:spotify-1+soundcloud-1".into()))?,
                tx.playlists().load(&PlaylistId::new("road"))?,
                tx.recently_played().list()?,
                tx.settings().get("audio.volume")?,
                tx.settings().get("ui.theme")?,
            ))
        })
        .unwrap();
    let (session, sequence, playlist, recent, volume, theme) = read;
    let session = session.unwrap();
    assert!(session.was_playing);
    assert_eq!(
        session.session,
        PlaybackSession {
            intent: Intent::Paused,
            ..c.session
        }
    );
    assert_eq!(sequence.unwrap(), c.sequence);
    assert_eq!(playlist.unwrap(), c.playlist);
    let recent: Vec<Track> = recent.into_iter().rev().map(|e| e.track).collect();
    assert_eq!(recent, c.recent);
    assert_eq!(
        (volume.as_deref(), theme.as_deref()),
        (Some(c.settings[0].1), Some(c.settings[1].1))
    );

    let read = storage
        .read_blocking(|tx| {
            Ok((
                tx.plugins().get(&"spotify".into())?,
                tx.connections().get(&"spotify-1".into())?,
                tx.grants().permissions(&"spotify".into())?,
                tx.grants().folders(&"local-1".into())?,
                tx.grants().server(&"spotify-1".into())?,
                tx.plugin_kv()
                    .get(&KvScope::Plugin("spotify".into()), "client-id")?,
                tx.plugin_kv()
                    .get(&KvScope::Connection("spotify-1".into()), "user")?,
                tx.receipts().get("party-1", "member-2:1")?,
            ))
        })
        .unwrap();
    let (plugin, connection, permissions, folders, server, client_id, user, receipt) = read;
    assert_eq!(plugin.unwrap(), c.plugin);
    assert_eq!(
        connection.unwrap(),
        ConnectionRecord {
            id: "spotify-1".into(),
            plugin: "spotify".into(),
            generation: 1,
            account: Some(c.account),
            created_ms: NOW,
            updated_ms: NOW,
        }
    );
    assert_eq!(permissions, c.permissions);
    assert_eq!(folders, c.folders);
    assert_eq!(server.unwrap(), c.server);
    assert_eq!(client_id.unwrap(), b"abc");
    assert_eq!(user.unwrap(), [0, 159, 255]);
    assert_eq!(receipt.as_deref(), Some("rejected: unavailable"));

    let (tracks, files, contents_) = storage
        .read_blocking(|tx| {
            let index = tx.local_index();
            Ok((
                index.tracks(&"local-1".into())?,
                index.files(&"local-1".into(), "content-1")?,
                index.contents(&"local-1".into())?,
            ))
        })
        .unwrap();
    assert_eq!(
        tracks,
        vec![Track {
            availability: Availability::Available,
            ..c.local_track
        }]
    );
    assert_eq!(files, c.local_files);
    assert_eq!(contents_, vec![c.local_content]);

    // Response sequences continue after what the fixture reserved.
    let next = storage
        .write_blocking(|tx| {
            tx.connections()
                .reserve_response_sequences(&"spotify".into(), &"spotify-1".into(), 1)
        })
        .unwrap();
    assert_eq!(next, 1001);
}

#[test]
fn schema_v1_is_frozen() {
    let fixture = tempfile::tempdir().unwrap();
    let fixture = load_fixture(fixture.path());
    let fresh = tempfile::tempdir().unwrap();
    let fresh = StoragePaths::new(fresh.path().join("data"), fresh.path().join("cache"));
    drop(Storage::open(&fresh, Arc::new(ManualClock::new(NOW))).unwrap());
    assert_eq!(
        schema_of(&fresh.database()),
        schema_of(&fixture.database()),
        "schema v1 changed; add a migration instead (src/migrate.rs)"
    );
}

#[test]
fn migrations_are_numbered_from_one_without_gaps() {
    for (i, migration) in crate::migrate::MIGRATIONS.iter().enumerate() {
        assert_eq!(migration.version as usize, i + 1);
    }
    assert_eq!(
        crate::migrate::MIGRATIONS.last().unwrap().version,
        crate::SCHEMA_VERSION
    );
}

mod runner {
    use super::*;
    use crate::migrate::{MIGRATIONS, Migrated, Migration, migrate, sibling};

    fn with(extra: Migration) -> Vec<Migration> {
        MIGRATIONS
            .iter()
            .map(|m| Migration {
                version: m.version,
                apply: m.apply,
            })
            .chain([extra])
            .collect()
    }

    #[test]
    fn a_new_migration_upgrades_the_fixture_and_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = load_fixture(dir.path());
        let before = dump(&Connection::open(paths.database()).unwrap());
        // A representative v2: rebuild a table, as SQLite's procedure does.
        let migrations = with(Migration {
            version: 2,
            apply: |tx| {
                tx.execute_batch(
                    "CREATE TABLE settings_new (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL,
                         changed_ms INTEGER) STRICT, WITHOUT ROWID;
                     INSERT INTO settings_new (key, value) SELECT key, value FROM settings;
                     DROP TABLE settings;
                     ALTER TABLE settings_new RENAME TO settings;",
                )
            },
        });
        let mut conn = Connection::open(paths.database()).unwrap();
        assert_eq!(
            migrate(&mut conn, &paths.database(), &migrations),
            Migrated::Ready { from: 1 }
        );
        assert_eq!(crate::migrate::user_version(&conn, "main").unwrap(), 2);
        let theme: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'ui.theme'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(theme, "dark");

        let backup = sibling(&paths.database(), ".v1.backup");
        assert_same_dump(&dump(&Connection::open(&backup).unwrap()), &before);
    }

    #[test]
    fn a_failing_migration_leaves_the_fixture_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let paths = load_fixture(dir.path());
        let before = dump(&Connection::open(paths.database()).unwrap());
        let migrations = with(Migration {
            version: 2,
            apply: |tx| {
                tx.execute_batch(
                    "ALTER TABLE settings ADD COLUMN extra TEXT; SELECT * FROM missing_table;",
                )
            },
        });
        let mut conn = Connection::open(paths.database()).unwrap();
        let outcome = migrate(&mut conn, &paths.database(), &migrations);
        assert!(
            matches!(outcome, Migrated::Failed { version: 1, .. }),
            "{outcome:?}"
        );
        drop(conn);
        assert_same_dump(&dump(&Connection::open(paths.database()).unwrap()), &before);
    }

    #[test]
    fn a_migration_that_breaks_a_foreign_key_is_rolled_back() {
        let dir = tempfile::tempdir().unwrap();
        let paths = load_fixture(dir.path());
        let before = dump(&Connection::open(paths.database()).unwrap());
        let migrations = with(Migration {
            version: 2,
            apply: |tx| tx.execute_batch("DELETE FROM plugins WHERE plugin_id = 'spotify';"),
        });
        let mut conn = Connection::open(paths.database()).unwrap();
        let outcome = migrate(&mut conn, &paths.database(), &migrations);
        assert!(
            matches!(outcome, Migrated::Failed { version: 1, .. }),
            "{outcome:?}"
        );
        drop(conn);
        assert_same_dump(&dump(&Connection::open(paths.database()).unwrap()), &before);
    }
}
