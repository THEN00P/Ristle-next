//! Storage behavior beyond the legacy suite: the storage threads,
//! row-level saves, the Plugin tables, the caches and recovery.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Dirs, detailed, reference, stored_sequence};
use ristle_domain::{ConnectionId, PlaylistId, PluginId, RistlePlaylist, Track};
use ristle_queue::sequence::{SequenceId, SlotId};
use ristle_queue::{
    CollectionPosition, Current, EntryId, Origin, QueueEntry, QueueState, RepeatMode,
};
use ristle_storage::repo::{
    Account, CachedResponse, ConfirmedOrigin, FolderGrant, HttpKey, Intent, KvScope, LocalContent,
    LocalFile, PlaybackSession, PluginRecord, ServerGrant,
};
use ristle_storage::{Changes, Error, OpenStatus, Storage};

fn plugin(id: &str) -> PluginRecord {
    PluginRecord {
        id: PluginId::new(id),
        version: "1.0.0".into(),
        package_hash: "sha256:00".into(),
        publisher_key: "ed25519:00".into(),
        enabled: true,
        installed_ms: 1,
        updated_ms: 1,
    }
}

/// A Plugin `spotify` with connections `spotify-1` and `spotify-2`.
fn with_connections(storage: &Storage) {
    storage
        .write_blocking(|tx| {
            tx.plugins().put(&plugin("spotify"))?;
            tx.connections()
                .create(&"spotify-1".into(), &"spotify".into())?;
            tx.connections()
                .create(&"spotify-2".into(), &"spotify".into())?;
            Ok(())
        })
        .unwrap();
}

fn collection_entry(id: u64, name: &str) -> QueueEntry {
    QueueEntry {
        id: EntryId(id),
        track: detailed("spotify", name),
        origin: Origin::Collection {
            position: CollectionPosition(id - 1),
        },
        slot: None,
        submitter: None,
    }
}

fn queue_of(count: u64) -> QueueState {
    QueueState {
        collection: (1..=count)
            .map(|i| collection_entry(i, &format!("t{i}")))
            .collect(),
        cursor: Some(0),
        current: Some(Current::AtCursor),
        next_id: EntryId(count + 1),
        ..QueueState::default()
    }
}

fn session(queue: QueueState) -> PlaybackSession {
    PlaybackSession {
        queue,
        sequence: None,
        position_ms: 0,
        intent: Intent::Paused,
    }
}

#[tokio::test]
async fn async_writes_are_visible_to_later_reads() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    storage
        .write(|tx| tx.settings().put("k", "v"))
        .await
        .unwrap();
    assert_eq!(
        storage.read(|tx| tx.settings().get("k")).await.unwrap(),
        Some("v".into())
    );
}

#[test]
fn writes_inside_a_read_are_refused() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let result = storage.read_blocking(|tx| tx.settings().put("k", "v"));
    assert!(matches!(result, Err(Error::ReadOnly)));
    assert_eq!(
        storage.read_blocking(|tx| tx.settings().get("k")).unwrap(),
        None
    );
}

#[test]
fn a_panicking_closure_rolls_back_and_the_storage_keeps_working() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let result = storage.write_blocking(|tx| {
        tx.settings().put("half", "written")?;
        panic!("injected");
        #[allow(unreachable_code)]
        Ok(())
    });
    assert!(matches!(result, Err(Error::Panicked)));
    storage
        .write_blocking(|tx| tx.settings().put("after", "ok"))
        .unwrap();
    let (half, after) = storage
        .read_blocking(|tx| Ok((tx.settings().get("half")?, tx.settings().get("after")?)))
        .unwrap();
    assert_eq!(half, None);
    assert_eq!(after, Some("ok".into()));
}

// Phase 2 M3 exit: a sequence save rewrites only the rows that changed.
#[test]
fn a_sequence_save_rewrites_only_the_rows_that_changed() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let mut sequence = stored_sequence("liked");
    let saved = sequence.clone();
    let first = storage
        .write_blocking(move |tx| tx.sequences().save(&saved))
        .unwrap();
    // The descriptor, one route, three slots and the progress row.
    assert_eq!(
        first,
        Changes {
            inserted: 6,
            updated: 0,
            deleted: 0
        }
    );

    let save = |state: &ristle_queue::SequenceState| {
        let state = state.clone();
        storage
            .write_blocking(move |tx| tx.sequences().save(&state))
            .unwrap()
    };
    assert!(
        save(&sequence).is_empty(),
        "an unchanged save writes nothing"
    );

    dirs.raw(
        "CREATE TRIGGER freeze_routes BEFORE UPDATE ON sequence_routes
             BEGIN SELECT RAISE(ABORT, 'route rewritten'); END;",
    );
    // A shuffle swaps the last two: two play indices and the progress row.
    sequence.order = vec![SlotId(0), SlotId(2), SlotId(1)];
    sequence.shuffle = Some(7);
    sequence.revision = 1;
    assert_eq!(
        save(&sequence),
        Changes {
            inserted: 0,
            updated: 3,
            deleted: 0
        }
    );

    // Resolving the positional slot rewrites that slot alone.
    sequence.slots[2].track = Some(reference("spotify", "third"));
    assert_eq!(
        save(&sequence),
        Changes {
            inserted: 0,
            updated: 1,
            deleted: 0
        }
    );

    // A re-base that drops a slot deletes it; the slot after it moves up in
    // natural order and keeps its play index.
    sequence.slots.remove(1);
    sequence.order = vec![SlotId(0), SlotId(2)];
    assert_eq!(
        save(&sequence),
        Changes {
            inserted: 0,
            updated: 1,
            deleted: 1
        }
    );

    let restored = storage
        .read_blocking(|tx| tx.sequences().load(&SequenceId("liked".into())))
        .unwrap()
        .unwrap();
    assert_eq!(restored, sequence);
}

#[test]
fn a_queue_save_rewrites_only_the_entries_that_changed() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let save = |queue: &QueueState| {
        let saved = session(queue.clone());
        storage
            .write_blocking(move |tx| tx.session().save(&saved))
            .unwrap()
    };
    let mut queue = queue_of(50);
    assert_eq!(save(&queue).inserted, 50);

    queue.cursor = Some(1);
    assert!(save(&queue).is_empty(), "advancing rewrote queue entries");

    queue.requests.push(QueueEntry {
        id: EntryId(51),
        origin: Origin::Request,
        ..collection_entry(51, "request")
    });
    queue.next_id = EntryId(52);
    assert_eq!(
        save(&queue),
        Changes {
            inserted: 1,
            updated: 0,
            deleted: 0
        }
    );

    // Removing the first entry shifts every later position.
    queue.collection.remove(0);
    queue.cursor = Some(0);
    assert_eq!(
        save(&queue),
        Changes {
            inserted: 0,
            updated: 49,
            deleted: 1
        }
    );

    let restored = storage
        .read_blocking(|tx| tx.session().load_for_cold_start())
        .unwrap()
        .unwrap();
    assert_eq!(restored.session.queue, queue);
}

#[test]
fn corrupt_queue_state_is_never_saved() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let mut queue = queue_of(2);
    queue.cursor = Some(5);
    let result = storage.write_blocking(move |tx| tx.session().save(&session(queue)));
    assert!(matches!(result, Err(Error::Invalid { .. })));
    assert_eq!(
        storage
            .read_blocking(|tx| tx.session().load_for_cold_start())
            .unwrap(),
        None
    );
}

#[test]
fn the_active_sequence_cannot_be_deleted_from_under_the_session() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    storage
        .write_blocking(|tx| {
            tx.sequences().save(&stored_sequence("active"))?;
            tx.session().save(&PlaybackSession {
                sequence: Some(SequenceId("active".into())),
                ..session(QueueState::default())
            })?;
            Ok(())
        })
        .unwrap();
    let result = storage.write_blocking(|tx| tx.sequences().delete(&SequenceId("active".into())));
    assert!(matches!(result, Err(Error::Database(_))), "{result:?}");
    assert!(
        storage
            .read_blocking(|tx| tx.sequences().load(&SequenceId("active".into())))
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_session_can_be_cleared() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    storage
        .write_blocking(|tx| {
            tx.session().save(&session(queue_of(3)))?;
            tx.session().clear()
        })
        .unwrap();
    assert_eq!(
        storage
            .read_blocking(|tx| tx.session().load_for_cold_start())
            .unwrap(),
        None
    );
    let saved = session(QueueState {
        repeat: RepeatMode::Queue,
        ..queue_of(1)
    });
    let expected = saved.queue.clone();
    storage
        .write_blocking(move |tx| tx.session().save(&saved))
        .unwrap();
    let restored = storage
        .read_blocking(|tx| tx.session().load_for_cold_start())
        .unwrap()
        .unwrap();
    assert_eq!(restored.session.queue, expected);
    assert_eq!(restored.saved_ms, 1_000);
}

#[test]
fn playlist_edits_rewrite_only_changed_positions() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let mut playlist = RistlePlaylist {
        id: PlaylistId::new("p"),
        title: "P".into(),
        tracks: (0..20)
            .map(|i| detailed("spotify", &format!("t{i}")))
            .collect(),
        created_ms: 1,
        updated_ms: 1,
    };
    let save = |playlist: &RistlePlaylist| {
        let playlist = playlist.clone();
        storage
            .write_blocking(move |tx| tx.playlists().save(&playlist))
            .unwrap()
    };
    assert_eq!(
        save(&playlist),
        Changes {
            inserted: 21,
            updated: 0,
            deleted: 0
        }
    );
    playlist.tracks.push(detailed("soundcloud", "added"));
    playlist.updated_ms = 2;
    assert_eq!(
        save(&playlist),
        Changes {
            inserted: 1,
            updated: 1,
            deleted: 0
        }
    );
    playlist.title = "Renamed".into();
    playlist.updated_ms = 3;
    assert_eq!(
        save(&playlist),
        Changes {
            inserted: 0,
            updated: 1,
            deleted: 0
        }
    );

    let (loaded, deleted, gone) = storage
        .write_blocking(|tx| {
            let loaded = tx.playlists().load(&PlaylistId::new("p"))?;
            let deleted = tx.playlists().delete(&PlaylistId::new("p"))?;
            Ok((loaded, deleted, tx.playlists().load(&PlaylistId::new("p"))?))
        })
        .unwrap();
    assert_eq!(loaded.unwrap(), playlist);
    assert!(deleted);
    assert_eq!(gone, None);
}

#[test]
fn plugin_kv_expires_and_enforces_one_quota_across_scopes() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    with_connections(&storage);
    let plugin_scope = KvScope::Plugin("spotify".into());
    let connection_scope = KvScope::Connection("spotify-1".into());

    storage
        .write_blocking({
            let (p, c) = (plugin_scope.clone(), connection_scope.clone());
            move |tx| {
                let kv = tx.plugin_kv();
                kv.set(&p, "client-id", b"0123456789", None, 64)?; // 19 bytes
                kv.set(&c, "token", &[7; 20], Some(10), 64)?; // 25 bytes
                Ok(())
            }
        })
        .unwrap();
    assert_eq!(
        storage
            .read_blocking(|tx| tx.plugin_kv().usage(&"spotify".into()))
            .unwrap(),
        44
    );

    // Replacing a value counts it once.
    let c = connection_scope.clone();
    storage
        .write_blocking(move |tx| tx.plugin_kv().set(&c, "token", &[8; 40], Some(10), 64))
        .unwrap();
    let c = connection_scope.clone();
    let over = storage.write_blocking(move |tx| tx.plugin_kv().set(&c, "more", &[0; 4], None, 64));
    assert!(
        matches!(
            over,
            Err(Error::QuotaExceeded {
                quota: 64,
                needed: 72
            })
        ),
        "{over:?}"
    );

    // After ten seconds the token is gone and no longer counted.
    dirs.clock.advance(10_000);
    let c = connection_scope.clone();
    assert_eq!(
        storage
            .read_blocking(move |tx| tx.plugin_kv().get(&c, "token"))
            .unwrap(),
        None
    );
    assert_eq!(
        storage
            .read_blocking(|tx| tx.plugin_kv().usage(&"spotify".into()))
            .unwrap(),
        19
    );
    assert_eq!(
        storage
            .write_blocking(|tx| tx.plugin_kv().purge_expired())
            .unwrap(),
        1
    );
    let p = plugin_scope.clone();
    assert_eq!(
        storage
            .read_blocking(move |tx| tx.plugin_kv().get(&p, "client-id"))
            .unwrap(),
        Some(b"0123456789".to_vec())
    );
}

#[test]
fn a_new_generation_stores_the_account_and_drops_connection_data() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    with_connections(&storage);
    let account = Account {
        id: "user-1".into(),
        name: "Ada".into(),
        avatar: vec![],
    };
    let generation = storage
        .write_blocking({
            let account = account.clone();
            move |tx| {
                for connection in ["spotify-1", "spotify-2"] {
                    tx.plugin_kv().set(
                        &KvScope::Connection(connection.into()),
                        "k",
                        b"v",
                        None,
                        1024,
                    )?;
                }
                tx.plugin_kv()
                    .set(&KvScope::Plugin("spotify".into()), "k", b"v", None, 1024)?;
                tx.connections()
                    .begin_generation(&"spotify-1".into(), Some(&account))
            }
        })
        .unwrap();
    assert_eq!(generation, 1);

    let get = |scope: KvScope| {
        storage
            .read_blocking(move |tx| tx.plugin_kv().get(&scope, "k"))
            .unwrap()
    };
    assert_eq!(get(KvScope::Connection("spotify-1".into())), None);
    assert!(get(KvScope::Connection("spotify-2".into())).is_some());
    assert!(get(KvScope::Plugin("spotify".into())).is_some());
    let record = storage
        .read_blocking(|tx| tx.connections().get(&"spotify-1".into()))
        .unwrap()
        .unwrap();
    assert_eq!(record.generation, 1);
    assert_eq!(record.account, Some(account));
}

#[test]
fn response_sequences_never_go_back() {
    let dirs = Dirs::new();
    let reserve = |storage: &Storage, connection: &'static str, count: u64| {
        storage
            .write_blocking(move |tx| {
                tx.connections().reserve_response_sequences(
                    &"spotify".into(),
                    &connection.into(),
                    count,
                )
            })
            .unwrap()
    };
    {
        let storage = dirs.open();
        with_connections(&storage);
        assert_eq!(reserve(&storage, "spotify-1", 100), 1);
        assert_eq!(reserve(&storage, "spotify-1", 100), 101);
        assert_eq!(
            reserve(&storage, "", 1),
            1,
            "the anonymous connection counts alone"
        );
        // Removing the connection keeps its counter.
        storage
            .write_blocking(|tx| tx.connections().remove(&"spotify-1".into()))
            .unwrap();
    }
    let storage = dirs.open();
    assert_eq!(reserve(&storage, "spotify-1", 1), 201);
}

#[test]
fn folder_and_server_grants_belong_to_their_connection() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    with_connections(&storage);
    let folders = vec![
        FolderGrant {
            id: "f1".into(),
            name: "Music".into(),
            locator: "content://tree/music".into(),
        },
        FolderGrant {
            id: "f2".into(),
            name: "Downloads".into(),
            locator: "content://tree/downloads".into(),
        },
    ];
    let server = ServerGrant {
        name: "Living Room".into(),
        urls: vec!["http://192.168.1.20:8096".into()],
        confirmed: vec![ConfirmedOrigin {
            origin: "http://192.168.1.20:8096".into(),
            public: false,
            addresses: vec!["192.168.1.20".into()],
        }],
    };
    storage
        .write_blocking({
            let (folders, server) = (folders.clone(), server.clone());
            move |tx| {
                tx.grants()
                    .set_permissions(&"spotify".into(), &["hosts".into()])?;
                tx.grants().set_folders(&"spotify-1".into(), &folders)?;
                tx.grants().set_server(&"spotify-1".into(), Some(&server))
            }
        })
        .unwrap();
    let (read_folders, read_server, other) = storage
        .read_blocking(|tx| {
            Ok((
                tx.grants().folders(&"spotify-1".into())?,
                tx.grants().server(&"spotify-1".into())?,
                tx.grants().folders(&"spotify-2".into())?,
            ))
        })
        .unwrap();
    assert_eq!(read_folders, folders);
    assert_eq!(read_server, Some(server));
    assert!(other.is_empty());

    // A duplicate folder id is refused; the earlier grant stays.
    let duplicate = vec![folders[0].clone(), folders[0].clone()];
    let result =
        storage.write_blocking(move |tx| tx.grants().set_folders(&"spotify-1".into(), &duplicate));
    assert!(matches!(result, Err(Error::Invalid { .. })), "{result:?}");

    // Removing the connection removes its grants, not the Plugin's.
    storage
        .write_blocking(|tx| tx.connections().remove(&"spotify-1".into()))
        .unwrap();
    let (server, permissions) = storage
        .read_blocking(|tx| {
            Ok((
                tx.grants().server(&"spotify-1".into())?,
                tx.grants().permissions(&"spotify".into())?,
            ))
        })
        .unwrap();
    assert_eq!(server, None);
    assert_eq!(permissions, vec!["hosts".to_owned()]);

    // Uninstalling removes everything of the Plugin.
    storage
        .write_blocking(|tx| tx.plugins().remove(&"spotify".into()))
        .unwrap();
    let (permissions, connections) = storage
        .read_blocking(|tx| Ok((tx.grants().all_permissions()?, tx.connections().list()?)))
        .unwrap();
    assert!(permissions.is_empty());
    assert!(connections.is_empty());
}

fn http_key(plugin: &str, connection: &str, key: &str) -> HttpKey {
    HttpKey {
        plugin: plugin.into(),
        connection: connection.into(),
        generation: 0,
        key: key.into(),
    }
}

#[test]
fn the_http_cache_evicts_by_age_and_least_recent_use() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let put = |key: HttpKey, bytes: usize, fetched_ms: u64| {
        storage
            .write_blocking(move |tx| {
                tx.http_cache().put(
                    &key,
                    &CachedResponse {
                        response: vec![0; bytes],
                        fetched_ms,
                    },
                )
            })
            .unwrap();
    };
    // Each entry is 99 bytes of response plus its 1-byte key.
    put(http_key("a", "c", "1"), 99, 10);
    dirs.clock.advance(1);
    put(http_key("a", "c", "2"), 99, 20);
    dirs.clock.advance(1);
    put(http_key("a", "c", "3"), 99, 30);
    dirs.clock.advance(1);
    put(http_key("b", "c", "1"), 99, 40);
    dirs.clock.advance(1);
    // Using the oldest makes it the most recent.
    assert!(
        storage
            .write_blocking(|tx| tx.http_cache().touch(&http_key("a", "c", "1")))
            .unwrap()
    );

    assert_eq!(
        storage
            .read_blocking(|tx| tx.http_cache().size(Some(&"a".into())))
            .unwrap(),
        300
    );
    let evicted = storage
        .write_blocking(|tx| tx.http_cache().evict_to_budget(Some(&"a".into()), 150))
        .unwrap();
    assert_eq!(evicted, 2);
    let present = |key: HttpKey| {
        storage
            .read_blocking(move |tx| tx.http_cache().get(&key))
            .unwrap()
            .is_some()
    };
    assert!(present(http_key("a", "c", "1")));
    assert!(!present(http_key("a", "c", "2")));
    assert!(!present(http_key("a", "c", "3")));
    assert!(
        present(http_key("b", "c", "1")),
        "another Plugin's budget was used"
    );

    assert_eq!(
        storage
            .write_blocking(|tx| tx.http_cache().evict_fetched_before(40))
            .unwrap(),
        1
    );
    assert!(!present(http_key("a", "c", "1")));
    assert_eq!(
        storage
            .write_blocking(|tx| tx.http_cache().clear_connection(&"b".into(), &"c".into()))
            .unwrap(),
        1
    );
}

#[test]
fn an_unreadable_cache_database_is_recreated_empty() {
    let dirs = Dirs::new();
    {
        let storage = dirs.open();
        storage
            .write_blocking(|tx| {
                tx.settings().put("kept", "yes")?;
                tx.http_cache().put(
                    &http_key("a", "c", "k"),
                    &CachedResponse {
                        response: vec![1],
                        fetched_ms: 1,
                    },
                )
            })
            .unwrap();
    }
    std::fs::write(
        dirs.paths.cache_database(),
        b"this is not a database at all, not even close",
    )
    .unwrap();
    for suffix in ["-wal", "-shm"] {
        let mut path = dirs.paths.cache_database().into_os_string();
        path.push(suffix);
        let _ = std::fs::remove_file(path);
    }

    let storage = dirs.open();
    assert_eq!(storage.status(), &OpenStatus::Ready);
    let (kept, cached) = storage
        .read_blocking(|tx| {
            Ok((
                tx.settings().get("kept")?,
                tx.http_cache().get(&http_key("a", "c", "k"))?,
            ))
        })
        .unwrap();
    assert_eq!(kept, Some("yes".into()));
    assert_eq!(cached, None);
}

#[test]
fn a_cache_database_of_another_version_is_recreated() {
    let dirs = Dirs::new();
    common::raw(
        &dirs.paths.cache_database(),
        "CREATE TABLE old (x); PRAGMA user_version = 7;",
    );
    let storage = dirs.open();
    storage
        .write_blocking(|tx| {
            tx.http_cache().put(
                &http_key("a", "c", "k"),
                &CachedResponse {
                    response: vec![1],
                    fetched_ms: 1,
                },
            )
        })
        .unwrap();
    drop(storage);
    let conn = rusqlite::Connection::open(dirs.paths.cache_database()).unwrap();
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let old: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old, 0);
}

#[test]
fn an_unreadable_database_opens_in_recovery_and_can_be_set_aside() {
    let dirs = Dirs::new();
    std::fs::create_dir_all(&dirs.paths.data_dir).unwrap();
    let garbage = vec![0x5a_u8; 8192];
    std::fs::write(dirs.paths.database(), &garbage).unwrap();

    let storage = dirs.open();
    assert!(matches!(storage.status(), OpenStatus::Recovery { .. }));
    assert_eq!(std::fs::read(dirs.paths.database()).unwrap(), garbage);
    let aside = storage.set_aside(&dirs.paths).unwrap();
    assert_eq!(std::fs::read(&aside).unwrap(), garbage);

    let fresh = dirs.open();
    assert_eq!(fresh.status(), &OpenStatus::Ready);
    // A ready database is never set aside.
    assert!(matches!(
        fresh.set_aside(&dirs.paths),
        Err(Error::Invalid { .. })
    ));
}

#[test]
fn connection_records_round_trip() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    with_connections(&storage);
    let connections = storage.read_blocking(|tx| tx.connections().list()).unwrap();
    let ids: Vec<&str> = connections.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["spotify-1", "spotify-2"]);
    assert!(
        connections
            .iter()
            .all(|c| c.generation == 0 && c.account.is_none() && c.created_ms == 1_000)
    );
    let missing = storage.write_blocking(|tx| {
        tx.connections()
            .begin_generation(&ConnectionId::new("nope"), None)
    });
    assert!(matches!(missing, Err(Error::Invalid { .. })));
    let plugins = storage.read_blocking(|tx| tx.plugins().list()).unwrap();
    assert_eq!(plugins, vec![plugin("spotify")]);
}

#[test]
fn storage_is_shared_across_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<Storage>();
}

// Codex review: a crash between the generation change in ristle.sqlite and
// the cache deletion must not serve the old account's responses.
#[test]
fn cached_rows_of_an_older_generation_are_never_read() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    with_connections(&storage);
    let old = HttpKey {
        generation: 0,
        ..http_key("spotify", "spotify-1", "me")
    };
    let put = old.clone();
    storage
        .write_blocking(move |tx| {
            tx.http_cache().put(
                &put,
                &CachedResponse {
                    response: b"old account".to_vec(),
                    fetched_ms: 1,
                },
            )
        })
        .unwrap();
    // The generation changes and the cache is not cleared, as after a crash
    // between the two commits.
    let generation = storage
        .write_blocking(|tx| tx.connections().begin_generation(&"spotify-1".into(), None))
        .unwrap();
    let current = HttpKey {
        generation,
        ..old.clone()
    };
    assert_eq!(
        storage
            .read_blocking(move |tx| tx.http_cache().get(&current))
            .unwrap(),
        None
    );
    // The old row is still cleared with the connection.
    assert_eq!(
        storage
            .write_blocking(|tx| tx
                .http_cache()
                .clear_connection(&"spotify".into(), &"spotify-1".into()))
            .unwrap(),
        1
    );
}

// Codex review: a database at the current version with a damaged schema
// opens in recovery instead of failing call by call.
#[test]
fn a_damaged_schema_at_the_current_version_opens_in_recovery() {
    let dirs = Dirs::new();
    drop(dirs.open());
    dirs.raw("DROP TABLE command_receipts;");
    let storage = dirs.open();
    let OpenStatus::Recovery { reason } = storage.status() else {
        panic!("a missing table was accepted");
    };
    assert!(reason.contains("command_receipts"), "{reason}");
}

#[test]
fn a_damaged_cache_schema_is_recreated() {
    let dirs = Dirs::new();
    drop(dirs.open());
    common::raw(
        &dirs.paths.cache_database(),
        "DROP INDEX http_cache_fetched;",
    );
    let storage = dirs.open();
    assert_eq!(storage.status(), &OpenStatus::Ready);
    drop(storage);
    let conn = rusqlite::Connection::open(dirs.paths.cache_database()).unwrap();
    let index: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'http_cache_fetched'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(index, 1);
}

// Codex review: the shown metadata follows the preferred file after the
// previously preferred file is reindexed as a worse one.
#[test]
fn local_track_metadata_follows_the_preferred_file() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    storage
        .write_blocking(|tx| {
            tx.plugins().put(&plugin("local-files"))?;
            tx.connections()
                .create(&"local-1".into(), &"local-files".into())?;
            Ok(())
        })
        .unwrap();
    let content = LocalContent {
        content_id: "c1".into(),
        fingerprint: "fp".into(),
        fingerprint_hash: 1,
        duration_ms: None,
    };
    let titled = |title: &str| Track {
        title: title.into(),
        ..detailed("local-files", "c1")
    };
    let file = |location: &str, format: &str, bitrate_kbps: u32| LocalFile {
        location: location.into(),
        content_id: "c1".into(),
        format: format.into(),
        bitrate_kbps,
        size_bytes: 1,
        modified_ms: 1,
        available: true,
    };
    let upsert = |track: Track, file: LocalFile| {
        let content = content.clone();
        storage
            .write_blocking(move |tx| {
                tx.local_index()
                    .upsert(&"local-1".into(), &content, &track, &file)
            })
            .unwrap();
    };
    let shown = || {
        storage
            .read_blocking(|tx| tx.local_index().tracks(&"local-1".into()))
            .unwrap()[0]
            .title
            .clone()
    };

    upsert(titled("From the FLAC"), file("a.flac", "flac", 900));
    upsert(titled("From the MP3"), file("b.mp3", "mp3", 320));
    assert_eq!(shown(), "From the FLAC");

    // The FLAC's location now holds a 128 kbps MP3: the 320 kbps MP3 wins.
    upsert(titled("Downgraded"), file("a.flac", "mp3", 128));
    assert_eq!(shown(), "From the MP3");

    // When the preferred file goes missing, the one that would play shows.
    storage
        .write_blocking(|tx| {
            let index = tx.local_index();
            index.begin_scan(&"local-1".into())?;
            index.confirm_file(&"local-1".into(), "a.flac", 1, 1)
        })
        .unwrap();
    assert_eq!(shown(), "Downgraded");
}

// Codex review: an id used again must not restart at a generation whose
// cached rows belong to the removed connection.
#[test]
fn a_reused_connection_id_continues_above_its_old_generations() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    with_connections(&storage);
    let generation = storage
        .write_blocking(|tx| {
            tx.connections()
                .begin_generation(&"spotify-1".into(), None)?;
            tx.connections().begin_generation(&"spotify-1".into(), None)
        })
        .unwrap();
    assert_eq!(generation, 2);
    let old = HttpKey {
        generation,
        ..http_key("spotify", "spotify-1", "me")
    };
    let put = old.clone();
    storage
        .write_blocking(move |tx| {
            tx.http_cache().put(
                &put,
                &CachedResponse {
                    response: b"old account".to_vec(),
                    fetched_ms: 1,
                },
            )?;
            tx.connections().remove(&"spotify-1".into())?;
            Ok(())
        })
        .unwrap();

    let reused = storage
        .write_blocking(|tx| {
            tx.connections()
                .create(&"spotify-1".into(), &"spotify".into())
        })
        .unwrap();
    assert_eq!(reused.generation, 3);
    let current = HttpKey {
        generation: reused.generation,
        ..old
    };
    assert_eq!(
        storage
            .read_blocking(move |tx| tx.http_cache().get(&current))
            .unwrap(),
        None
    );
    // And it keeps counting from there.
    let next = storage
        .write_blocking(|tx| tx.connections().begin_generation(&"spotify-1".into(), None))
        .unwrap();
    assert_eq!(next, 4);
}
