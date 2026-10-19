# Phase 2 plan: the Rust core

Written 2026-10-19 for review before the implementation threads start.
Phase 2 exits when CI is green and every Ristle legacy test case is ported or
explicitly retired (`HANDOFF.md`). This file is the plan for getting there and
the checklist for the exit.

How to read it: a thread that owns a milestone reads [Conventions](#1-conventions-for-every-crate),
its crate in [Crate layout](#2-crate-layout), the [interfaces](#3-interfaces-fixed-up-front)
it implements or calls, its [milestone](#4-milestones), and its rows in the
[test-port inventory](#8-test-port-inventory). The [legacy traps](#6-legacy-traps-and-where-the-design-answers-them)
apply to M2, M3, M7 and M8. Nothing here depends on the conversation that
produced it.

Inputs: ADRs 0001, 0003, 0004, 0007, 0009, 0011, 0012; `plugin-api/wit/` and
`docs/plugin-api/`; the research notes for spikes B, C and D; and Ristle
legacy's `packages/core`, `packages/app-core`, `packages/audio-engine` and
`packages/party-client`.

## 1. Conventions for every crate

**Toolchain and lints.** Rust 1.99.1 is pinned in `core/rust-toolchain.toml`
(wasmtime ≥ 49 needs ≥ 1.96). Edition 2024. The workspace lints in
`core/Cargo.toml` apply to every crate: `missing_docs`, `unreachable_pub`,
`unsafe_code = "deny"`, and clippy's `unwrap_used`, `expect_used`, `panic`,
`todo`, `dbg_macro` and `print_*`. CI runs clippy with `-D warnings`, so
these fail the build. `core/clippy.toml` exempts tests. A module that needs
`unsafe` (JNI bootstrap, MediaCodec, AAudio, UniFFI glue) allows it locally
and gives every block a `// SAFETY:` comment.

**Errors.** Each crate has one public `Error` enum built with `thiserror`. No
`anyhow` in library crates. Inside `ristle-plugin-host`, use
`wasmtime::Result` where wasmtime requires it (its `Error` does not convert
to or from `anyhow`). `ristle-app` maps every crate error to its own error,
and `ristle-ffi` maps that to `CoreError`. Nothing panics across the UniFFI
boundary (ADR 0001); every fallible exported function returns `Result`.

**Async and threads.** tokio. `ristle-app` owns the one multi-thread runtime
and hands out handles. `domain`, `queue` and `storage` are synchronous.
`storage` runs on one dedicated thread that owns the write connection;
callers send it closures and await the result. The audio render callback never
locks, allocates or logs. Observers are always notified from the runtime,
never inline on the caller's thread (ADR 0001).

**Time and randomness.** Wall-clock time comes from `ristle_domain::Clock`.
No crate calls `SystemTime::now()` except the system clock implementation.
Every random order takes an explicit `u64` seed, and the seed is persisted
with the order it produced. Tests use a manual clock and fixed seeds.

**Logging.** `tracing`. The facade forwards to the platform log.

**Tests.**

- A ported legacy test keeps the legacy name in snake_case, or the name the
  inventory gives, so `docs/plan/legacy_tests.py progress` can find it.
- Port the behavior, not the C++ structure. Rewrite the setup against the
  Rust API and keep every assertion. If an assertion no longer makes sense,
  the row becomes `retire` with a reason, in the same PR.
- Unit tests live next to the code. Scenarios that cross crates live in
  `crates/<crate>/tests/`.
- No test touches the internet. HTTP tests run against a local server on
  127.0.0.1; `ristle-net` provides one behind its `test-support` feature.
- Media fixtures are short synthetic tones encoded by a committed script
  (`crates/media/tests/fixtures/generate.sh`), each under 200 KB, so their
  licence is ours.
- Plugin tests load real components built from `plugin-api/examples/`
  (see M5). Components are encoded in the test helper with the
  `wit-component` crate, so tests need no CLI tools.

**Dependencies.** Add a dependency in the PR that first uses it, declare it in
`[workspace.dependencies]`, and keep `cargo deny` green. This set was checked
against `core/deny.toml` on 2026-10-19 and passes: wasmtime 49 (Cranelift,
Pulley, component model, cache), wasmsign2 0.2.8, rusqlite 0.37 (bundled),
reqwest 0.12 on rustls, tokio, symphonia 0.6.1, symphonia-adapter-libopus,
cpal 0.18, rubato 0.16, aes/ctr/cbc, uniffi 0.32.2, serde, toml, thiserror,
tracing, ed25519-dalek, sha2, ndk, jni, ndk-context, rand_chacha.
`symphonia-adapter-fdk-aac`, `fdk-aac` and `fdk-aac-sys` are banned by name:
`fdk-aac-sys` declares MIT and would otherwise pass.

**Pull requests.** One PR per milestone slice, owned by one workstream. CI is
green, the behavior is exercised by tests, and the description says what was
not verified (AGENTS.md). A PR that changes an interface in
[section 3](#3-interfaces-fixed-up-front) edits this file in the same PR.

## 2. Crate layout

All crates live in `core/crates/`, are named `ristle-<dir>`, and are
GPL-3.0-or-later (ADR 0009).

| Crate | Responsibility | Depends on | Milestone |
|---|---|---|---|
| `domain` | Track identity and Track, Provider and Provider Connection ids, Media Date, artwork, Ristle Playlist, collection descriptors, `Clock`. No I/O | (none) | M1 |
| `queue` | Canonical Queue; playable sequences: slots, shuffle permutation, windows, hydration plan, drift re-base. Pure and seeded | `domain` | M2 |
| `storage` | rusqlite, migrations with recovery mode, all repositories, `ristle.sqlite` and `cache.sqlite` | `domain`, `queue` | M3 |
| `net` | The one HTTP client, request policy, HTTP cache semantics, per-host rate limits | `domain` | M4 |
| `plugin-host` | Package verification, wasmtime engine, instances, every WIT import, typed wrappers for every export | `domain`, `net` | M5 |
| `media` | Stream Source to PCM: progressive, HLS, Byte Source, decryption, demux (MP4 with edit lists, TS, ADTS, Ogg), decoder registry | `domain`, `net` | M6 |
| `audio` | Engine (gapless ring, transport, seek sequence, gain, ended state) and output (cpal, AAudio, capture) | `domain`, `media` | M6 |
| `protocol` | Client-server types shared with `ristle-server` (ADR 0001, ADR 0007) | `domain` | M8 |
| `party` | Party Host command application, Party State projection, guest resend, File Share policy | `domain`, `queue`, `protocol` | M8 |
| `app` | Composition root: session, playback control, sequences and refill, library, Unified Search, observers, port implementations | all of the above | M7 |
| `ffi` | UniFFI facade, library name `ristle_core`. Conversion only | `app`, `domain` | M1, M7 |

```
domain ◄─ queue ◄─ storage
   ▲   ◄─ net ◄─ plugin-host
   │         ◄─ media ◄─ audio
   └── protocol ◄─ party (also queue)

app ──► every crate above        ffi ──► app, domain
```

**Ports.** A crate that needs persistence, secrets or the platform declares
the trait it needs, and `ristle-app` implements it over `ristle-storage` or
the facade's platform callbacks. So `net`, `plugin-host`, `media`, `audio`
and `party` never depend on `storage`, and their tests use in-memory
implementations shipped behind each crate's `test-support` feature. The
adapters in `app` are thin and each has a test against a real database.

Deviations from the expected list, and why:

1. **`net` is its own crate.** The Plugin host's `http` import and the media
   fetcher must share one connection pool and one rate-limit view per host.
   When a Provider answers a Plugin's request with 429, stream fetches to the
   same host must wait too.
2. **Audio is two crates.** `media` (fetch, demux, decode) is testable on
   fixture files without an output device and holds most of the risk (HLS,
   TS, edit lists, platform decoders). `audio` is real-time code with other
   rules. Two threads can work on them at once.
3. **Orchestration and facade are separate.** `app` is tested in plain Rust;
   `ffi` only converts. The Qt desktop app (phase 4) adds a cxx-qt facade over
   the same `app` in `apps/desktop`.
4. **`protocol` and `party` exist in phase 2.** AGENTS.md puts Party logic in
   the core, and legacy has 40 Party tests that need no server. Phase 2
   builds them against an in-memory Relay. See open question 1.
5. **No search crate.** Unified Search is a module of `app`: it needs the
   Provider fan-out and the clock, and is a few hundred lines.
6. **The Local Files Plugin is not a core crate.** It is a component like any
   other (ADR 0003, ADR 0010) and lives in `plugins/local-files/`,
   GPL-3.0-or-later, built in M7b. The mock and example Plugins live in
   `plugin-api/examples/` (MIT) and are built in M5.

## 3. Interfaces fixed up front

Names and shapes below are binding. Bodies, private helpers and additional
methods are the owning thread's call. Changing a listed signature means
editing this section in the same PR and telling the coordinator, because
another workstream builds against it.

### `ristle-domain` (M1, first PR; every workstream waits on it)

```rust
pub struct ProviderId(String);          // the Plugin's manifest id
pub struct ConnectionId(String);
pub struct PluginId(String);

/// Track identity. `connection` is set only for Providers whose ids are
/// server-scoped (Server Grants); catalog Tracks compare without it.
pub struct TrackRef { provider: ProviderId, id: String, connection: Option<ConnectionId> }
impl TrackRef {
    pub fn catalog(provider: ProviderId, id: String) -> Result<Self, InvalidTrackRef>;
    pub fn server_scoped(provider: ProviderId, id: String, connection: ConnectionId)
        -> Result<Self, InvalidTrackRef>;
}

pub struct Track {
    pub reference: TrackRef,
    pub title: String,
    pub artists: Vec<Named>,
    pub album: Option<Named>,
    pub duration_ms: Option<u64>,
    pub artwork: Vec<Artwork>,
    pub released: Option<MediaDate>,
    pub uploaded: Option<MediaDate>,
    pub capabilities: PlaybackCapabilities,  // seekable, live
    pub availability: Availability,          // Available | Unavailable { reason } | Unknown
    pub url: Option<String>,
    pub isrc: Option<String>,
    pub attributes: Vec<MetadataAttribute>,
}

pub struct MediaDate { pub year: u16, pub month: Option<u8>, pub day: Option<u8> }

/// How a Provider collection is read (walkthrough, "Spotify Liked Songs").
pub struct CollectionRef { pub connection: ConnectionId, pub id: String }
pub enum Addressing { Cursor, Offset }

pub trait Clock: Send + Sync + 'static { fn now_ms(&self) -> u64; }
```

### `ristle-queue` (M2)

The state types come first, in M2's first PR, because `storage` persists
them.

```rust
pub struct EntryId(u64);
pub enum Origin { Collection { position: CollectionPosition }, Request }
/// Where an entry sits in its collection. Persisted, so a continuation is
/// anchored by position, never by counting duplicates (legacy trap 6).
pub struct CollectionPosition(u64);
pub struct Submitter { pub id: String, pub display_name: String }
pub struct QueueEntry { pub id: EntryId, pub track: Track, pub origin: Origin,
                        pub slot: Option<SlotId>, pub submitter: Option<Submitter> }
pub enum RepeatMode { Off, Track, Queue }
pub enum Advance { TrackEnded, Skip }

/// Serializable, restores exactly; `CanonicalQueue::restore` rejects corrupt
/// state (duplicate ids, cursor out of range).
pub struct QueueState { /* collection, cursor, requests, play_next_count, current,
                           repeat, shuffle (seed + original order), next_id */ }

impl CanonicalQueue {
    pub fn start_collection(&mut self, entries: Vec<CollectionEntry>, start: usize) -> StartOutcome;
    pub fn add_to_queue(&mut self, track: Track, by: Option<Submitter>) -> Result<EntryId, Rejection>;
    pub fn play_next(&mut self, track: Track, by: Option<Submitter>) -> Result<EntryId, Rejection>;
    pub fn append_collection(&mut self, entries: Vec<CollectionEntry>) -> AddOutcome;
    pub fn replace_upcoming_collection(&mut self, entries: Vec<CollectionEntry>) -> AddOutcome;
    pub fn advance(&mut self, how: Advance) -> bool;
    pub fn advance_to(&mut self, entry: EntryId) -> bool;
    pub fn go_back(&mut self) -> bool;
    pub fn remove(&mut self, entry: EntryId) -> Option<Removed>;
    pub fn undo_remove(&mut self, removed: Removed) -> bool;
    pub fn move_request(&mut self, entry: EntryId, to: usize) -> bool;
    pub fn clear_upcoming(&mut self) -> usize;
    pub fn prune(&mut self, tracks: &[TrackRef], keep_current: Option<EntryId>);
    pub fn trim_history(&mut self, keep: usize) -> usize;
    pub fn set_repeat(&mut self, mode: RepeatMode);
    pub fn shuffle_upcoming(&mut self, seed: u64);
    pub fn restore_upcoming_order(&mut self);
    pub fn snapshot(&self) -> QueueState;
    pub fn restore(state: QueueState) -> Result<Self, CorruptState>;
}

/// A collection walked without fetching it whole.
pub enum Slot { Resolved { entry: SlotId, track: TrackRef }, Position { entry: SlotId, offset: u64 } }
pub struct SequenceState { /* descriptor, slots, order, cursor, materialized_end,
                              shuffle seed, played-in-this-pass, revision */ }
impl Sequence {
    pub fn start(descriptor: SequenceDescriptor, slots: Vec<Slot>, at: StartAt, shuffle: Option<u64>) -> Result<Self, InvalidSequence>;
    pub fn set_shuffle(&mut self, seed: Option<u64>);   // keeps current and consumed prefix
    pub fn advance(&mut self) -> bool;
    pub fn advance_to(&mut self, slot: SlotId) -> bool;
    pub fn go_back(&mut self) -> bool;
    pub fn restart(&mut self);
    pub fn window(&self, history: usize, lookahead: usize) -> Vec<SlotId>;
    pub fn hydration_plan(&self, max: usize) -> Vec<SlotId>;     // only unresolved lookahead
    pub fn resolve(&mut self, resolved: Vec<(SlotId, TrackRef)>);
    pub fn apply_local_change(&mut self, change: CollectionChange); // like/unlike made in Ristle
    pub fn rebase(&mut self, page: ObservedPage) -> RebaseOutcome;  // drift found on re-read
    pub fn state(&self) -> &SequenceState;
    pub fn restore(state: SequenceState) -> Result<Self, CorruptState>;
}
```

### `ristle-storage` (M3)

```rust
pub enum OpenStatus { Ready, Recovery { reason: String } }
impl Storage {
    pub fn open(paths: &StoragePaths, clock: Arc<dyn Clock>) -> Result<Self, Error>;
    pub fn status(&self) -> &OpenStatus;
    /// Runs on the storage thread inside one transaction.
    pub async fn write<T: Send + 'static>(&self, f: impl FnOnce(&mut Tx<'_>) -> Result<T, Error> + Send + 'static) -> Result<T, Error>;
    pub async fn read<T: Send + 'static>(&self, f: impl FnOnce(&Tx<'_>) -> Result<T, Error> + Send + 'static) -> Result<T, Error>;
}
// Repositories hang off `Tx`: session(), sequences(), playlists(), recently_played(),
// settings(), plugins(), grants(), connections(), plugin_kv(), receipts(),
// local_index(), page_cache(), http_cache(), party() (added in M8).
```

Schema v1, `ristle.sqlite`: `playback_session`, `queue_entries` (with
`collection_position`, `origin`, `submitter_*`), `sequences`,
`sequence_slots`, `sequence_routes`, `sequence_progress`, `ristle_playlists`,
`ristle_playlist_entries`, `recently_played`, `settings`, `plugins` (package
hash, Publisher key id, version), `plugin_grants` (Plugin Permissions,
Folder Grants, Server Grants), `provider_connections` (Credential Record
metadata; secrets live in the platform store), `connection_data` and
`plugin_kv` (with TTL), `response_sequences` (the WIT `response.sequence`,
which must never go back, so it is not cache), `command_receipts`,
`local_files`, `local_tracks`. `cache.sqlite`: `http_cache`, `page_cache`.
Compiled Plugin artifacts are files in the cache directory keyed by package
hash and engine configuration.

### Ports

```rust
// ristle-net
pub trait HttpCacheStore: Send + Sync {
    fn get(&self, key: &CacheKey) -> Result<Option<StoredResponse>, StoreError>;
    fn put(&self, key: &CacheKey, response: &StoredResponse) -> Result<(), StoreError>;
    fn remove(&self, key: &CacheKey) -> Result<(), StoreError>;
    fn evict_older_than(&self, last_fetched_before_ms: u64) -> Result<u64, StoreError>;
}

// ristle-plugin-host
pub trait PluginStore: Send + Sync {
    fn plugin_kv(&self, plugin: &PluginId) -> Box<dyn KvScope>;
    fn connection_kv(&self, connection: &ConnectionId) -> Box<dyn KvScope>;
    fn next_response_sequence(&self, connection: &ConnectionId) -> Result<u64, StoreError>;
    fn grants(&self, plugin: &PluginId) -> Result<Grants, StoreError>;
}
pub trait SecretVault: Send + Sync {  // implemented by the facade's PlatformHost
    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, VaultError>;
    fn write(&self, key: &str, value: &[u8]) -> Result<(), VaultError>;
    fn delete(&self, key: &str) -> Result<(), VaultError>;
}

// ristle-media
#[async_trait] pub trait ByteSourceReader: Send + Sync {
    async fn read(&self, handle: &ByteSourceHandle, offset: u64, length: u32) -> Result<Bytes, SourceError>;
}
pub trait MediaStream: Send {                 // pulled by the audio engine's decode thread
    fn format(&self) -> StreamFormat;          // may change once (implicit SBR)
    fn read(&mut self, out: &mut [f32]) -> Result<Read, MediaError>;  // Frames(n) | Pending | End
    fn seek(&mut self, position_ms: u64) -> Result<(), MediaError>;  // presentation time, after priming
}
pub trait DecoderProvider: Send + Sync {
    fn supports(&self, codec: &CodecDescription) -> Support;
    fn open(&self, codec: &CodecDescription) -> Result<Box<dyn Decoder>, MediaError>;
}

// ristle-audio
pub trait AudioOutput: Send {
    fn open(&mut self, request: OutputRequest, render: Box<dyn Render>) -> Result<OutputInfo, OutputError>;
    fn pause(&mut self); fn resume(&mut self); fn close(&mut self);
}

// ristle-app: what the session needs from a Provider. Implemented over
// ristle-plugin-host in M7b, and by fakes in M7a tests.
#[async_trait] pub trait ProviderPort: Send + Sync {
    async fn list(&self, collection: &CollectionRef, request: ListRequest) -> Result<Listing, ProviderError>;
    async fn tracks(&self, connection: &ConnectionId, ids: Vec<String>) -> Result<Vec<Result<Track, ProviderError>>, ProviderError>;
    async fn search(&self, connection: &ConnectionId, request: SearchRequest) -> Result<SearchResults, ProviderError>;
    async fn resolve(&self, track: &TrackRef, quality: Quality) -> Result<StreamSource, ProviderError>;
    async fn set_liked(&self, item: &ItemRef, liked: bool) -> Result<(), ProviderError>;
}
/// `ProviderError::kind()` decides playback behavior: Unplayable, Account and
/// Unknown are reported and skipped; Transient stops on the Track, paused.

// ristle-party
#[async_trait] pub trait RelayLink: Send + Sync {
    async fn send(&self, message: protocol::ToRelay) -> Result<(), LinkError>;
    async fn next(&self) -> Result<protocol::FromRelay, LinkError>;
}
pub trait HostQueue {  // implemented by ristle-app; one storage transaction per command
    fn apply(&mut self, member: &MemberId, seq: u64, command: protocol::Command) -> Applied; // Ok | Rejected(reason) | Retry
    fn projection(&self, budget: &Budget) -> protocol::QueueWindow;
}
```

### UniFFI facade shape (M1)

Fixed in M1 so phase 3 can design against it. Conventions from ADR 0001
apply: `CancelToken` on long calls, `Result` everywhere, callbacks on core
threads, library-mode generation.

```rust
#[derive(uniffi::Object)] pub struct RistleCore { /* Arc<ristle_app::Core> */ }

#[uniffi::export]
impl RistleCore {
    #[uniffi::constructor]
    pub fn new(config: CoreConfig, platform: Arc<dyn PlatformHost>) -> Result<Arc<Self>, CoreError>;
    pub fn subscribe(&self, observer: Arc<dyn CoreObserver>) -> Arc<Subscription>;
    pub fn snapshot(&self) -> CoreSnapshot;                 // now playing, queue window, transport

    // Transport and queue: return at once; effects arrive through the observer.
    pub fn play(&self) -> Result<(), CoreError>;
    pub fn pause(&self) -> Result<(), CoreError>;
    pub fn next(&self) -> Result<(), CoreError>;
    pub fn previous(&self) -> Result<(), CoreError>;
    pub fn seek(&self, position_ms: u64) -> Result<(), CoreError>;
    pub fn set_repeat(&self, mode: RepeatMode) -> Result<(), CoreError>;
    pub fn set_shuffle(&self, enabled: bool) -> Result<(), CoreError>;
    pub fn set_volume(&self, volume: f32) -> Result<(), CoreError>;
    pub fn add_to_queue(&self, items: Vec<ItemRef>, placement: Placement) -> Result<(), CoreError>;
    pub fn remove_entry(&self, entry: u64) -> Result<(), CoreError>;
    pub fn undo_removal(&self) -> Result<(), CoreError>;
    pub fn move_request(&self, entry: u64, to: u32) -> Result<(), CoreError>;
    pub fn clear_upcoming(&self) -> Result<(), CoreError>;
    pub fn jump_to(&self, entry: u64) -> Result<(), CoreError>;
    pub fn handle_media_command(&self, command: MediaCommand) -> Result<(), CoreError>;

    // Reading and starting: async, cancellable.
    pub async fn play_source(&self, source: PlaySource, start: StartAt, shuffle: bool, cancel: Arc<CancelToken>) -> Result<(), CoreError>;
    pub async fn search(&self, query: String, scope: SearchScope, cancel: Arc<CancelToken>) -> Result<SearchSnapshot, CoreError>;
    pub async fn open_page(&self, target: PageTarget, refresh: bool, cancel: Arc<CancelToken>) -> Result<Page, CoreError>;
    pub async fn list(&self, collection: CollectionRef, cursor: Option<String>, cancel: Arc<CancelToken>) -> Result<ListingPage, CoreError>;
    pub async fn library(&self, refresh: bool, cancel: Arc<CancelToken>) -> Result<Library, CoreError>;
    pub async fn set_liked(&self, item: ItemRef, liked: bool) -> Result<(), CoreError>;

    // Ristle Playlists and history.
    pub fn ristle_playlists(&self) -> Result<Vec<PlaylistSummary>, CoreError>;
    pub fn create_playlist(&self, title: String) -> Result<PlaylistSummary, CoreError>;
    pub fn rename_playlist(&self, playlist: String, title: String) -> Result<(), CoreError>;
    pub fn delete_playlist(&self, playlist: String) -> Result<(), CoreError>;
    pub async fn playlist_add(&self, destination: PlaylistDestination, items: Vec<ItemRef>, duplicates: Duplicates, cancel: Arc<CancelToken>) -> Result<PlaylistAddOutcome, CoreError>;
    pub fn recently_played(&self) -> Result<Vec<TrackView>, CoreError>;

    // Plugins, Provider Connections and sign-in (an Authorization Session).
    pub fn plugins(&self) -> Result<Vec<PluginView>, CoreError>;
    pub async fn review_package(&self, package: Vec<u8>) -> Result<InstallReview, CoreError>;
    pub async fn install(&self, review: String, consent: Consent) -> Result<PluginView, CoreError>;
    pub async fn begin_sign_in(&self, plugin: String, cancel: Arc<CancelToken>) -> Result<SignInStep, CoreError>;
    pub async fn continue_sign_in(&self, session: String, answer: SignInAnswer, cancel: Arc<CancelToken>) -> Result<SignInStep, CoreError>;
    pub async fn sign_out(&self, connection: String) -> Result<(), CoreError>;
    pub fn plugin_settings(&self, connection: String) -> Result<Vec<SettingsGroup>, CoreError>;
    pub async fn apply_setting(&self, connection: String, key: String, value: SettingValue) -> Result<Option<String>, CoreError>;
}

#[uniffi::export(with_foreign)]
pub trait CoreObserver: Send + Sync {
    fn on_snapshot(&self, snapshot: CoreSnapshot);         // revisioned, monotonic
    fn on_notice(&self, notice: PlaybackNotice);
    fn on_library_changed(&self, change: LibraryChange);
}

#[uniffi::export(with_foreign)]
pub trait PlatformHost: Send + Sync {                      // platform concerns only (AGENTS.md)
    fn read_secret(&self, key: String) -> Result<Option<Vec<u8>>, PlatformError>;
    fn write_secret(&self, key: String, value: Vec<u8>) -> Result<(), PlatformError>;
    fn delete_secret(&self, key: String) -> Result<(), PlatformError>;
}
```

Sign-in steps the UI must show (a web view with capture rules, a device
code, folder or server choice) are `SignInStep` variants, answered through
`continue_sign_in`. The core never calls into UI.

## 4. Milestones

Each milestone ends with its inventory rows ported
(`legacy_tests.py progress` shows none pending for it) and the exit test
listed. Counts are ported legacy tests.

```
M0 ─► M1 ─┬─► M2 ─► M3 ─┬─► M7a ─┬─► M7b ─► M9
          │              │        └─► M8 ──┘
          ├─► M4 ─► M5 ──┼──────────► M7b
          └─► M6a, M6b ──┴──────────► M7b
```

**M0. Foundation** (this PR). Workspace, crate stubs, lints, `deny.toml`,
the `core` CI workflow, this plan, `legacy_tests.py`.
Exit: CI green on the PR.

**M1. Domain types and facade shape.** `ristle-domain` complete (section 3).
`ristle-ffi` with the full facade shape: records, enums, `CoreObserver`,
`PlatformHost`, `CoreError`, `CancelToken`, `Subscription`. Each method calls
the `ristle_app::Core` method of the same name with its final signature;
until its milestone lands, that method returns `CoreError::NotReady { feature }`,
and `ristle-ffi` keeps a test listing the methods that still do. A
`core/tools/uniffi-bindgen` binary and a CI job that generates Kotlin and
Swift in library mode from an unstripped build and checks
`cargo ndk -t arm64-v8a check -p ristle-ffi`.
Ports: `track_test.cpp` (3).
Exit: bindings generate in CI; the facade shape is reviewed by the user
before phase 3 designs against it; `spikes/d-uniffi` deleted.

**M2. Queue and sequences.** `ristle-queue`. First PR: the state types.
Then the Canonical Queue and `sequence` per section 3, including the
walkthrough's offset-collection rules (shuffle without fetching, resolve
before showing, local likes applied, re-base on drift).
Ports: `canonical_queue_test.cpp`, `playable_sequence_test.cpp` (48).
Exit: all M2 rows; property tests (every shuffle is a permutation of the
unconsumed slots; `restore(snapshot())` is identity); one test per walkthrough
drift step; and `replace_upcoming_resumes_after_the_right_duplicate_far_apart`
(the same Track 60 positions apart, legacy trap 6).

**M3. Storage.** `ristle-storage`: schema v1, migration runner with recovery
mode, the storage thread, every repository, both databases.
Ports: `storage_test.cpp` and two `application_core_test.cpp` cases (21).
Exit: all M3 rows; a committed schema-v1 fixture database that every later
migration is tested against; a test that a sequence save rewrites only the
rows that changed.

**M4. Net.** `ristle-net`: client, request policy (forbidden headers from
`host.wit`, response header cap 64 KiB and 200 headers, body caps), HTTP
cache (Cache-Control, Plugin override with TTL, stale window and key,
stale-if-error only, 30-day retention, no background revalidation),
per-host rate limits (429, `Retry-After`, Plugin-reported limits).
Exit: a test for each rule above against the local test server.

**M5. Plugin host.** `ristle-plugin-host` per ADR 0003 and
`docs/plugin-api/packaging.md`: manifest and `wasmsign2` verification,
install, update and takeover by Publisher; one engine configuration with the
small-memory settings on every platform; Cranelift, or Pulley where no JIT is
allowed; epoch deadlines; compiled-artifact cache; instance per Provider
Connection, recycled after a trap or memory growth; every import checked
against manifest and consent; typed wrappers for every export, returning
`ristle-domain` types. `plugin-api/examples/mock-provider` implements
`full-provider` with scripted behavior; `plugin-api/examples/example-provider`
is the documented example. A `plugin-api` CI job builds both.
Needs M4 for `http`; packages, engine and instances can start in parallel
with M4.
Exit: conformance tests against the mock Plugin for every import and export,
run under Cranelift and with Pulley forced; tamper, wrong-key and takeover
tests; a trap fails one call and the next call succeeds on a fresh instance;
an endless loop is stopped by the epoch deadline. `spikes/c-wasm-android` and
`spikes/c-wasm-ios` deleted, with the `c` job of `spike-c-ios.yml`.

**M6. Media and audio**, two parallel tracks.
*M6a, `ristle-media`:* every item of ADR 0004 and its amendment, and one test
per "Host requirements from the spike B review" bullet. MediaCodec is the
first platform AAC provider; AudioToolbox and Media Foundation follow in
phases 6 and 4.
*M6b, `ristle-audio`:* engine against fake `MediaStream`s first, then the
cpal output and Android's AAudio output (PowerSaving, music content type,
`ndk-context` bootstrap) and a capture output for tests.
Ports: `audio_engine_test.cpp` (8).
Exit: all M6 rows; sample-exact gapless across MP3 (LAME), AAC with `elst`,
Vorbis, Opus and FLAC fixtures; HLS tests against the local server for TS,
fMP4, byte-range seeks, `EXT-X-BYTERANGE`, AES-128 including `EXT-X-MAP`,
muxed `traf` filtering and multi-`trun` fragments; Android cross-build in CI.
A run on the emulator is recorded in the PR (CI cannot play audio).
`spikes/b-audio` deleted.

**M7a. App: session and sequences.** `ristle-app` with the session (queue,
sequence and storage in one transaction), cold-start restore paused, command
receipts, Ristle Playlists and their active-playlist reconciliation, Recently
Played, sequences and paged sources with background refill, composites,
Unified Search, observers. Providers are `ProviderPort` fakes.
Ports: 66 cases from `application_core_test.cpp`, `app_core_test.cpp`,
`provider_queue_page_test.cpp` and `unified_search_test.cpp`.
Exit: all M7a rows.

**M7b. App: playback and Plugins.** The `ProviderPort` adapter over the Plugin
host; playback control (open current, unplayable and transient rules,
Previous restart threshold, prepare next for gapless); likes; pages, library
and refresh with caching; Authorization Sessions; Local Track grouping and the
Local Files Plugin (`plugins/local-files/`); every port implemented over
storage; the facade's `NotReady` list emptied.
Ports: 8 cases from `app_core_test.cpp`.
Exit: all M7b rows; an end-to-end test from the facade: mock Plugin search,
start a collection, the capture output receives the expected PCM, restart
restores paused at the same position.

**M8. Party core.** `ristle-protocol` (can start after M1) and `ristle-party`
per ADR 0007 and ADR 0012, with an in-memory Relay.
Ports: 40 Party cases.
Exit: all M8 rows; a test with one host and two guests over the in-memory
Relay that drops and reorders messages and still applies each command once.

**M9. Phase exit.** `legacy_tests.py check` and `progress --strict` pass;
no facade method returns `NotReady`; bindings and the Android build run in
CI; spikes B, C and D are gone; `HANDOFF.md` moves to phase 3.

## 5. Workstreams

Each workstream is one thread at a time and owns its crates. Nobody edits
another workstream's crate except through the interfaces in section 3.

| Workstream | Milestones | Owns | Can start | Needs from others |
|---|---|---|---|---|
| A. Domain and queue | M1 (domain), M2 | `domain`, `queue` | now | none |
| B. Storage | M3 | `storage` | after A's first two PRs (domain types, queue state types); the migration runner can start at once | state types from A |
| C. Net and Plugin host | M4, M5 | `net`, `plugin-host`, `plugin-api/examples/` | after the domain PR | domain types |
| D. Media | M6a | `media` | now (file sources first) | `net` client from C for HTTP sources |
| E. Audio engine | M6b | `audio` | now, against fake `MediaStream`s | `MediaStream` from D (fixed in section 3) |
| F. Facade and app | M1 (facade), M7a, M7b | `ffi`, `app`, `plugins/local-files/` | facade now; M7a after M2 and M3 | everything, through section 3 |
| G. Party | M8 | `protocol`, `party` | protocol after M1; party after M7a | `HostQueue` from F |

At the start, A, C, D, E and F run in parallel; B joins within days. G
starts when M7a lands and runs alongside M7b.

Shared files: `core/Cargo.toml` (append to `[workspace.dependencies]` only;
on a conflict keep both lines), `Cargo.lock` (regenerate on rebase),
`deny.toml` (changes need the user, since it is the licence policy), and this
file.

## 6. Legacy traps and where the design answers them

From Ristle legacy's `CLAUDE.md`. Each has a test that guards it.

1. **Positional Liked Songs.** Spotify's Liked Songs are offset-addressed;
   every like shifts every position. The Plugin declares
   `addressing: offset` and the host owns positions as `Slot::Position`,
   resolves before showing or playing, applies local likes, and re-bases on
   drift without reshuffling (`queue::sequence`, walkthrough steps 1-4). Never
   assume an unresolved slot names a Track.
2. **Shuffle spanning three layers.** Legacy split shuffle between
   TypeScript, C++ and the Plugins. Here it lives in `queue::sequence` (the
   permutation) and `app` (the session), and no UI holds order. Keep legacy's
   property that batches group by source position, not by index into the
   order vector, so hydration reads stay page-local
   (`shuffle_walks_eight_random_source_slices_per_large_batch`).
3. **The first-shuffle path.** Legacy started Liked Songs as a rolling merged
   queue and upgraded it to a sequence on the first shuffle, a separate code
   path. Here Liked Songs is a sequence from the first play, so the first
   shuffle takes the same path as every later one
   (`shuffle_covers_the_full_liked_sequence_without_restarting_current_track`,
   `liked_sequence_shuffle_preserves_queued_request_lanes`).
4. **Composites.** Liked Songs across Providers and search results are
   composites. Their id, refill key and title derive from their members,
   never from a constant
   (`sequence_catalog_namespaces_composite_occurrences_and_keeps_positional_providers_first`).
5. **Routing ids.** A queue source carries the `CollectionRef` used to read it
   (Spotify routes as `album:<id>` or `playlist:<id>`) and, separately, the
   entity the user opened, for display and navigation. Two fields, two types.
6. **Collection-entry ordinals.** Legacy anchored a continuation by counting a
   Track's occurrences within 50 entries of history, so duplicates more than
   50 apart resumed at the wrong one. Every collection entry here persists its
   `CollectionPosition`, which is the fix legacy deferred.
7. **Unplayable versus transient.** An unplayable Track never stops the queue
   and a network failure never skips one. Plugin errors carry a kind; the
   session skips Unplayable, Account and Unknown (backwards for Previous, with
   a limit in a row) and stops paused on Transient.
8. **Party command answers.** An unresolvable Track or a bad argument is a
   rejection in Party State; only a transient host failure leaves a command
   pending (ADR 0007, `HostQueue::apply` returns `Retry`).
9. **Legacy's known slow paths.** Routes are stored once per sequence, not
   copied per entry, and progress writes only changed rows. Sequence
   preparation returns a token that the start must present, replacing
   legacy's correlation by occurrence identity
   (`rejects_superseded_identical_sequence_preparation`).

## 7. Spike code

Nothing is copied. Code below is rewritten into the named crate with tests and
CI, using the spike as a reference that was measured to work. A spike
directory is deleted in the PR that closes its milestone; the deletion
commit message names the last commit that contains it, for `git show`.

| Spike | Moves to | What | Not moved | Deleted |
|---|---|---|---|---|
| `d-uniffi` | `ffi`, `app`, `core/tools/uniffi-bindgen` | `CancelToken` and the abort-on-drop guard, running async bodies on the core runtime, `Subscription`, observer dispatch through the runtime, `shutdown_background` on drop, the bindgen binary | Android and iOS demo apps (phase 3 and 6 build real ones; JNA `CallbackThreadInitializer` and off-main construction are recorded in ADR 0001) | M1 exit, with the `d` job of `spike-c-ios.yml` |
| `c-wasm-android` | `plugin-host`, `net` | Engine configuration and small-memory settings, artifact cache keyed by hash and config, the host `http` import | `pkg/` (its own signature format; packaging chose `wasmsign2`), `sc-plugin/` (belongs to the SoundCloud Plugin repository, ADR 0010), memory measurement | M5 exit |
| `c-wasm-ios` | `plugin-host` | Pulley selection and the iOS memory settings, which are the settings on every platform | XCTest and Firebase harness (phase 6 builds production iOS tests) | M5 exit, with the `c` job |
| `b-audio` | `media`, `audio` | `hls.rs`, `hlsseek.rs` → `media::hls`; `ts.rs` → `media::demux::ts` (300-400 lines with tests); `mp4edit.rs` → `media::demux::mp4`; `registry.rs`, `decode.rs` → `media::decode`; `mediacodec.rs` → `media::decode::mediacodec`; `gapless.rs` → test helpers; `player/` → `audio` (SPSC ring, rubato, cpal, engine); `android.rs` → `audio::output::android`; `gen-media.sh` → the fixture script, synthetic sources only | `cputime.rs`, `server.rs`, soak and bench scripts, the Kotlin app | M6 exit |
| `a-qt-quick` | (phase 4) | Not touched in phase 2. Phase 4 moves the frame-timing harness and the Lumen QML | | Phase 4 |

## 8. Test-port inventory

Every test case in Ristle legacy's `packages/core/tests`,
`packages/app-core/tests`, `packages/audio-engine/tests` and
`packages/party-client` (its unit and integration tests), 219 in all:
194 port, 21 retire, 4 phase 5. The tables are the source of truth;
`docs/plan/legacy_tests.py` reads them.

- **port:** the Rust test with the given name must exist in the given crate
  by the end of the milestone.
- **retire:** the behavior no longer exists, with the reason.
- **phase 5:** the behavior exists but needs the server or the network
  transport. These rows stay open after phase 2 (open question 2).

```sh
RISTLE_LEGACY=../Ristle python docs/plan/legacy_tests.py check     # every legacy test is placed
python docs/plan/legacy_tests.py progress                           # what is still pending, by milestone
python docs/plan/legacy_tests.py progress --strict                  # the phase exit
```

Changing a row (a rename, a retirement found while porting) happens in the
PR that does the port, with the reason.

Other legacy Rust suites are reference material, not exit criteria, because
their code is replaced by the WIT design or moves to the Plugin repositories:
`provider-media` (14 tests, HLS and AES: M6a reads them and ports what still
applies), `provider-host` (17), `provider-host-core` (9),
`provider-wasm-host` (8), `mobile-provider-host` (7), `provider-wasm-*` and
`provider-protocol-rust` (9), and the SoundCloud (45) and Spotify (32)
Plugins, which go to their own repositories (ADR 0010).

### `packages/core/tests/track_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `TrackIdentityIgnoresProviderConnection` | port | `domain` | M1 | `track_identity_ignores_provider_connection` |
| `TrackIdentityKeepsProvidersSeparate` | port | `domain` | M1 | `track_identity_keeps_providers_separate` |
| `InvalidTrackIdentityIsRejected` | port | `domain` | M1 | `invalid_track_identity_is_rejected` |

### `packages/core/tests/canonical_queue_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `CollectionPlaybackSilentlySkipsUnavailableTracks` | port | `queue` | M2 | `collection_playback_silently_skips_unavailable_tracks` |
| `StartingAtDisabledTrackUsesNextAvailableTrack` | port | `queue` | M2 | `starting_at_disabled_track_uses_next_available_track` |
| `DirectQueueRequestRejectsUnavailableTrack` | port | `queue` | M2 | `direct_queue_request_rejects_unavailable_track` |
| `QueuedRequestsPlayFifoBeforeCollectionResumes` | port | `queue` | M2 | `queued_requests_play_fifo_before_collection_resumes` |
| `RepeatTrackAlsoRepeatsQueuedRequest` | port | `queue` | M2 | `repeat_track_also_repeats_queued_request` |
| `RepeatQueueLoopsUnderlyingCollection` | port | `queue` | M2 | `repeat_queue_loops_underlying_collection` |
| `RepeatQueueWrapExcludesTheCurrentEntry` | port | `queue` | M2 | `repeat_queue_wrap_excludes_the_current_entry` |
| `StartingNewCollectionPreservesRepeatMode` | port | `queue` | M2 | `starting_new_collection_preserves_repeat_mode` |
| `ShuffleReordersOnlyUpcomingCollectionTracks` | port | `queue` | M2 | `shuffle_reorders_only_upcoming_collection_tracks` |
| `DisablingShuffleRestoresOriginalUpcomingOrder` | port | `queue` | M2 | `disabling_shuffle_restores_original_upcoming_order` |
| `ShufflePreservesForwardRequestPriority` | port | `queue` | M2 | `shuffle_preserves_forward_request_priority` |
| `HistoryCollectsAdvancedCollectionTracksInOrder` | port | `queue` | M2 | `history_collects_advanced_collection_tracks_in_order` |
| `HistoryExcludesQueuedRequestsAndResetsOnNewCollection` | port | `queue` | M2 | `history_excludes_queued_requests_and_resets_on_new_collection` |
| `GoBackReturnsToPreviousCollectionTrack` | port | `queue` | M2 | `go_back_returns_to_previous_collection_track` |
| `ReplaceUpcomingCollectionPreservesCurrentAndQueuedRequests` | port | `queue` | M2 | `replace_upcoming_collection_preserves_current_and_queued_requests` |
| `ReplaceUpcomingCollectionPreservesSelectedContinuationAfterRequest` | port | `queue` | M2 | `replace_upcoming_collection_preserves_selected_continuation_after_request` |
| `ReplaceUpcomingCollectionMatchesTheDuplicatedContinuation` | port | `queue` | M2 | `replace_upcoming_collection_matches_the_duplicated_continuation` |
| `ReplaceUpcomingCollectionDropsDeletedContinuationAfterRequest` | port | `queue` | M2 | `replace_upcoming_collection_drops_deleted_continuation_after_request` |
| `AdvanceToJumpsDirectlyToUpcomingEntry` | port | `queue` | M2 | `advance_to_jumps_directly_to_upcoming_entry` |
| `AdvanceToPreservesEarlierQueuedRequestsAsHistory` | port | `queue` | M2 | `advance_to_preserves_earlier_queued_requests_as_history` |
| `AdvanceToDoesNotDuplicatePreservedCurrentRequest` | port | `queue` | M2 | `advance_to_does_not_duplicate_preserved_current_request` |
| `AdvanceToPreservesRequestHistoryBeforeWrappedTarget` | port | `queue` | M2 | `advance_to_preserves_request_history_before_wrapped_target` |
| `EnablingRepeatAfterSelectionDoesNotReplayPreservedRequests` | port | `queue` | M2 | `enabling_repeat_after_selection_does_not_replay_preserved_requests` |
| `UndoRestoresPreservedRequestInCollection` | port | `queue` | M2 | `undo_restores_preserved_request_in_collection` |
| `CollectionQueueingPreservesOrderAndSkipsDisabledTracks` | port | `queue` | M2 | `collection_queueing_preserves_order_and_skips_disabled_tracks` |
| `DuplicateTracksRemainDistinctQueueEntries` | port | `queue` | M2 | `duplicate_tracks_remain_distinct_queue_entries` |
| `QueueStateRoundTripsWhilePlayingQueuedRequest` | port | `queue` | M2 | `queue_state_round_trips_while_playing_queued_request` |
| `QueueStateRoundTripsWithPreservedRequestAfterCursor` | port | `queue` | M2 | `queue_state_round_trips_with_preserved_request_after_cursor` |
| `QueueRestoreRejectsDuplicateEntryIds` | port | `queue` | M2 | `queue_restore_rejects_duplicate_entry_ids` |
| `PagedCollectionHistoryCanBeTrimmedWithoutLosingItsCursor` | port | `queue` | M2 | `paged_collection_history_can_be_trimmed_without_losing_its_cursor` |
| `PlayNextPrecedesFifoQueueAndRetainsRequestOrder` | port | `queue` | M2 | `play_next_precedes_fifo_queue_and_retains_request_order` |
| `AppendingAProviderPageDeduplicatesExactTrackIdentities` | port | `queue` | M2 | `appending_a_provider_page_deduplicates_exact_track_identities` |
| `AppendingAProviderPageIgnoresPreservedRequestHistory` | port | `queue` | M2 | `appending_a_provider_page_ignores_preserved_request_history` |
| `QueueRemovalCanBeUndoneAtItsOriginalPosition` | port | `queue` | M2 | `queue_removal_can_be_undone_at_its_original_position` |
| `QueuedRequestsCanMoveAndUpcomingCanClear` | port | `queue` | M2 | `queued_requests_can_move_and_upcoming_can_clear` |
| `PruningExpiredTrackOccurrencesPreservesOnlyTheActiveOccurrence` | port | `queue` | M2 | `pruning_expired_track_occurrences_preserves_only_the_active_occurrence` |
| `PruningAPlayNextEntryKeepsTheFollowingRequestOutOfThePrefix` | port | `queue` | M2 | `pruning_a_play_next_entry_keeps_the_following_request_out_of_the_prefix` |
| `PruningFinalCurrentOccurrenceDoesNotLeaveAQueueReference` | port | `queue` | M2 | `pruning_final_current_occurrence_does_not_leave_a_queue_reference` |

### `packages/core/tests/playable_sequence_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `SequenceManifestPreservesDuplicateTrackOccurrences` | port | `queue` | M2 | `sequence_manifest_preserves_duplicate_track_occurrences` |
| `SequenceManifestRejectsDuplicateOccurrenceIds` | port | `queue` | M2 | `sequence_manifest_rejects_duplicate_occurrence_ids` |
| `ShufflePlayPermutesEveryOccurrenceExactlyOnce` | port | `queue` | M2 | `shuffle_play_permutes_every_occurrence_exactly_once` |
| `ShuffleWalksEightRandomSourceSlicesPerLargeBatch` | port | `queue` | M2 | `shuffle_walks_eight_random_source_slices_per_large_batch` |
| `ShuffleInterleavesEachRoundAcrossItsSourcePages` | port | `queue` | M2 | `shuffle_interleaves_each_round_across_its_source_pages` |
| `EnablingShuffleKeepsCurrentAndConsumedPrefixStable` | port | `queue` | M2 | `enabling_shuffle_keeps_current_and_consumed_prefix_stable` |
| `DisablingShuffleRestoresNaturalUnconsumedOrder` | port | `queue` | M2 | `disabling_shuffle_restores_natural_unconsumed_order` |
| `SequenceWindowBoundsHydratedHistoryAndLookahead` | port | `queue` | M2 | `sequence_window_bounds_hydrated_history_and_lookahead` |
| `SequenceHydrationPlanContainsOnlyMissingLookahead` | port | `queue` | M2 | `sequence_hydration_plan_contains_only_missing_lookahead` |
| `SequenceOrderRejectsCorruptPersistedPermutation` | port | `queue` | M2 | `sequence_order_rejects_corrupt_persisted_permutation` |

### `packages/core/tests/storage_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `StorageCreatesCurrentSchema` | port | `storage` | M3 | `storage_creates_current_schema` |
| `PlaybackSessionRestoresPausedWithExactQueueState` | port | `storage` | M3 | `playback_session_restores_paused_with_exact_queue_state` |
| `QueueSubmitterSurvivesTheRoomThatNamedIt` | port | `storage` | M3 | `queue_submitter_survives_the_party_that_named_it` |
| `RistlePlaylistPreservesMixedAndUnavailableTracks` | port | `storage` | M3 | `ristle_playlist_preserves_mixed_and_unavailable_tracks` |
| `RecentlyPlayedUsesDurationThresholdAndKeepsLatestHundred` | port | `storage` | M3 | `recently_played_uses_duration_threshold_and_keeps_latest_hundred` |
| `ClearCachePreservesUserOwnedPlaylists` | port | `storage` | M3 | `clear_cache_preserves_user_owned_playlists` |
| `ClearProviderCacheOnlyRemovesTheRequestedNamespace` | port | `storage` | M3 | `clear_provider_cache_only_removes_the_requested_namespace` |
| `SettingsPersistAndUpsertByKey` | port | `storage` | M3 | `settings_persist_and_upsert_by_key` |
| `RetiredPlaybackSettingsArePurgedDuringMigration` | retire | | | Purges settings of legacy schema 3. Ristle starts a new database at schema 1 and imports nothing (open question 3). |
| `LocalTrackCandidatesPreferFlacThenBitrateAndFallBack` | port | `storage` | M3 | `local_track_candidates_prefer_flac_then_bitrate_and_fall_back` |
| `LocalTrackCandidateOnlyRevalidatesWhenFileStateMatches` | port | `storage` | M3 | `local_track_candidate_only_revalidates_when_file_state_matches` |
| `FailedMigrationPreservesPriorDatabaseAndEntersRecovery` | port | `storage` | M3 | `failed_migration_preserves_prior_database_and_enters_recovery` |
| `NewerDatabaseSchemaEntersRecoveryWithoutModification` | port | `storage` | M3 | `newer_database_schema_enters_recovery_without_modification` |
| `PlayableSequenceManifestAndExactOrderRoundTrip` | port | `storage` | M3 | `playable_sequence_manifest_and_exact_order_round_trip` |
| `SequenceProgressDoesNotRewriteUnchangedOccurrenceOrder` | port | `storage` | M3 | `sequence_progress_does_not_rewrite_unchanged_occurrence_order` |
| `DeletingPlayableSequenceManifestCascadesItsOrder` | port | `storage` | M3 | `deleting_playable_sequence_manifest_cascades_its_order` |
| `FrozenSequenceRoutesAndIncrementalProgressSurviveRestartAtomically` | port | `storage` | M3 | `frozen_sequence_routes_and_incremental_progress_survive_restart_atomically` |
| `VersionOneLocalIndexMigratesWithoutDroppingRows` | retire | | | Migrates legacy schema 1's local index. No legacy database is opened (open question 3); the migration mechanism is covered by the two recovery tests. |
| `PlaybackStateFailureRollsBackReplacementManifestAndExactOrder` | port | `storage` | M3 | `playback_state_failure_rolls_back_replacement_manifest_and_exact_order` |
| `PlaybackStateFailureDoesNotAdvanceDurableSequenceOrder` | port | `storage` | M3 | `playback_state_failure_does_not_advance_durable_sequence_order` |
| `PlaybackReceiptLimitRejectsNewCommandsWithoutEvictingDedupeHistory` | port | `storage` | M3 | `playback_receipt_limit_rejects_new_commands_without_evicting_dedupe_history` |

### `packages/core/tests/unified_search_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `SearchWaitsForProvidersBeforeDeadline` | port | `app` | M7a | `search_waits_for_providers_before_deadline` |
| `SearchFinalizesAsSoonAsEveryProviderResponds` | port | `app` | M7a | `search_finalizes_as_soon_as_every_provider_responds` |
| `SearchFreezesAtFiveSecondDeadline` | port | `app` | M7a | `search_freezes_at_five_second_deadline` |
| `LateSearchResultsCannotReorderFrozenSnapshot` | port | `app` | M7a | `late_search_results_cannot_reorder_frozen_snapshot` |
| `SearchGroupsKindsAndInterleavesProviders` | port | `app` | M7a | `search_groups_kinds_and_interleaves_providers` |
| `SearchRejectsProviderMismatches` | port | `app` | M7a | `search_rejects_provider_mismatches` |

### `packages/core/tests/application_core_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `ApplicationCoreAlwaysRestoresPersistedMixedPlaylistOnColdStart` | port | `app` | M7a | `always_restores_persisted_mixed_playlist_on_cold_start` |
| `ApplicationCoreMaterializesOnlyAWindowOfLargeRistlePlaylist` | port | `app` | M7a | `materializes_only_a_window_of_large_ristle_playlist` |
| `ApplicationCoreStartsRistlePlaylistInACompleteShuffledOrder` | port | `app` | M7a | `starts_ristle_playlist_in_a_complete_shuffled_order` |
| `ApplicationCorePersistsExactLargePlaylistShuffleAcrossRestart` | port | `app` | M7a | `persists_exact_large_playlist_shuffle_across_restart` |
| `ApplicationCoreRefillsLargeSequenceWithoutGrowingCanonicalQueue` | port | `app` | M7a | `refills_large_sequence_without_growing_canonical_queue` |
| `ApplicationCoreHydratesProviderSequenceInBoundedWindows` | port | `app` | M7a | `hydrates_provider_sequence_in_bounded_windows` |
| `ApplicationCoreRejectsSupersededIdenticalSequencePreparation` | port | `app` | M7a | `rejects_superseded_identical_sequence_preparation` |
| `ApplicationCoreRestoresProviderSequenceWithoutLegacyPageToken` | port | `app` | M7a | `restores_provider_sequence_from_its_persisted_order_alone` |
| `ApplicationCoreResumesProviderSequenceAfterRapidlyReachingHydratedTail` | port | `app` | M7a | `resumes_provider_sequence_after_rapidly_reaching_hydrated_tail` |
| `ApplicationCoreRefillWalksPastAnUnavailableHydrationBatch` | port | `app` | M7a | `refill_walks_past_an_unavailable_hydration_batch` |
| `ApplicationCoreDoesNotReplayOccurrencesMovedIntoTheRequestLane` | port | `app` | M7a | `does_not_replay_occurrences_moved_into_the_request_lane` |
| `ApplicationCoreKeepsPlayingSequenceWhenAPreparedStartCannotBegin` | port | `app` | M7a | `keeps_playing_sequence_when_a_prepared_start_cannot_begin` |
| `ApplicationCoreKeepsPlayingSequenceWhenAPlaylistCannotStart` | port | `app` | M7a | `keeps_playing_sequence_when_a_playlist_cannot_start` |
| `ApplicationCoreStartsPastAnUnavailableInitialPlaylistWindow` | port | `app` | M7a | `starts_past_an_unavailable_initial_playlist_window` |
| `ApplicationCoreReplaysAnUndoneOccurrenceItAlreadyPassed` | port | `app` | M7a | `replays_an_undone_occurrence_it_already_passed` |
| `ApplicationCoreShufflesTheEntireActiveProviderSequence` | port | `app` | M7a | `shuffles_the_entire_active_provider_sequence` |
| `ApplicationCoreKeepsTheSelectedOccurrenceWhenShuffleRebuilds` | port | `app` | M7a | `keeps_the_selected_occurrence_when_shuffle_rebuilds` |
| `ApplicationCoreRejectsARefillPlannedBeforeShuffle` | port | `app` | M7a | `rejects_a_refill_planned_before_shuffle` |
| `ApplicationCoreRepeatQueueRestartsTheEntireProviderPermutation` | port | `app` | M7a | `repeat_queue_restarts_the_entire_provider_permutation` |
| `ApplicationCoreRepeatQueueRestartsAfterUnavailableSequenceTail` | port | `app` | M7a | `repeat_queue_restarts_after_unavailable_sequence_tail` |
| `ApplicationCoreLikedSequenceUpgradePreservesQueuedRequestLanes` | port | `app` | M7a | `liked_sequence_shuffle_preserves_queued_request_lanes` |
| `ActiveRistlePlaylistEditsReconcileThePersistedUpcomingQueue` | port | `app` | M7a | `active_ristle_playlist_edits_reconcile_the_persisted_upcoming_queue` |
| `ActivePlaylistEditPreservesContinuationAfterRequestHistory` | port | `app` | M7a | `active_playlist_edit_preserves_continuation_after_request_history` |
| `DeletingTheActiveRistlePlaylistDetachesItsQueueSource` | port | `app` | M7a | `deleting_the_active_ristle_playlist_detaches_its_queue_source` |
| `ApplicationCorePersistsProviderCacheEntries` | port | `storage` | M3 | `provider_page_cache_entries_survive_reopen` |
| `ApplicationCorePersistsPluginPermissionGrants` | port | `storage` | M3 | `plugin_permission_grants_survive_reopen` |
| `ApplicationCoreQueuesRequestBeforePlaylistContinuation` | port | `app` | M7a | `queues_request_before_playlist_continuation` |
| `ApplicationCorePreviewsQueueMovesWithoutMutatingSession` | port | `app` | M7a | `previews_queue_moves_without_mutating_session` |
| `ApplicationCorePersistsDirectQueueSelection` | port | `app` | M7a | `persists_direct_queue_selection` |
| `ApplicationCorePersistsRequestHistoryAfterDirectSelection` | port | `app` | M7a | `persists_request_history_after_direct_selection` |
| `ApplicationCoreGoBackReturnsToPreviousTrackAndResetsPlayhead` | port | `app` | M7a | `go_back_returns_to_previous_track_and_resets_playhead` |
| `ApplicationCoreKeepsProviderLikedSongsOutOfLocalPlaylists` | retire | | | Guards against legacy's retired `liked-songs` Ristle Playlist row. Likes are Provider-owned through the `likes` export; no such row can exist in a new database. |
| `ApplicationCoreRejectsDisabledDirectQueueRequest` | port | `app` | M7a | `rejects_disabled_direct_queue_request` |
| `ApplicationCorePersistsTransportAndRepeatState` | port | `app` | M7a | `persists_transport_and_repeat_state` |
| `ApplicationCoreSkipLeavesRepeatedTrackButTrackEndReplaysIt` | port | `app` | M7a | `skip_leaves_repeated_track_but_track_end_replays_it` |
| `ApplicationCorePersistsQueueEditingAndUndo` | port | `app` | M7a | `persists_queue_editing_and_undo` |
| `ApplicationCoreRefillsAndRestoresPagedQueueSources` | port | `app` | M7a | `refills_and_restores_paged_queue_sources` |
| `ClearingUpcomingTracksDetachesPagedQueueSource` | port | `app` | M7a | `clearing_upcoming_tracks_detaches_paged_queue_source` |
| `PersistsProviderLibraryQueueSourceKind` | port | `app` | M7a | `persists_provider_library_queue_source` |
| `PersistsProviderSearchQueueSourceKind` | port | `app` | M7a | `persists_provider_search_queue_source` |
| `PagedQueueKeepsBoundedPlaybackHistory` | port | `app` | M7a | `paged_queue_keeps_bounded_playback_history` |
| `ApplicationCoreReturnsTransientLikedSongMutationResults` | retire | | | Tests legacy's transitional Liked Songs API that answered without persisting a Ristle Playlist. Likes are Plugin calls (`likes.set-liked`) and are tested in M7b. |
| `PlaybackCommandReceiptSurvivesRestartAndPreventsRepeatedMutation` | port | `app` | M7a | `playback_command_receipt_survives_restart_and_prevents_repeated_mutation` |
| `PlaybackMutationFailureRestoresMemoryDatabaseRoutingAndReceipt` | port | `app` | M7a | `playback_mutation_failure_restores_memory_database_routing_and_receipt` |
| `InvalidOrdinaryStartsPreserveActiveSequenceAcrossRestart` | port | `app` | M7a | `invalid_ordinary_starts_preserve_active_sequence_across_restart` |

### `packages/app-core/tests/app_core_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `PartyGuestDeparturePrunesDuplicatesAndLetsCurrentFinish` | port | `party` | M8 | `party_guest_departure_prunes_duplicates_and_lets_current_finish` |
| `PartyOfferEntersQueueBeforeAudioAndUnreadyOccurrenceIsDeferred` | port | `party` | M8 | `party_offer_enters_queue_before_audio_and_unready_occurrence_is_deferred` |
| `DirectQueueSelectionStartsSlowTargetOnTheFirstRequest` | port | `app` | M7b | `direct_queue_selection_starts_slow_target_on_the_first_request` |
| `DirectQueueSelectionPreservesSkippedRequestsForPrevious` | port | `app` | M7a | `direct_queue_selection_preserves_skipped_requests_for_previous` |
| `ConsecutiveDirectQueueSelectionsDoNotDuplicateRequests` | port | `app` | M7a | `consecutive_direct_queue_selections_do_not_duplicate_requests` |
| `DirectQueueSelectionHistoryRestoresAfterGoingBack` | port | `app` | M7a | `direct_queue_selection_history_restores_after_going_back` |
| `UndoRestoresPreservedRequestAfterDirectSelection` | port | `app` | M7a | `undo_restores_preserved_request_after_direct_selection` |
| `PreviousRestartMarkerExpiresPastTheRestartThreshold` | port | `app` | M7b | `previous_restart_marker_expires_past_the_restart_threshold` |
| `ProviderSequenceCachesUseDurableInternalSettings` | retire | | | Legacy kept core state in a settings map the UI could read. Sequence caches are typed tables in `ristle-storage`, covered by its sequence round-trip tests. |
| `PartyBootstrapSettingsAreDurableInternalSettings` | retire | | | Same as above for Party bootstrap data, which lives in typed Party tables and is covered by the M8 restart tests. |
| `EveryQueueMutationNotifiesThePlatformSink` | port | `app` | M7a | `every_queue_mutation_notifies_observers` |
| `PlatformPlaybackSnapshotsHaveMonotonicRevisions` | port | `app` | M7a | `published_snapshots_have_monotonic_revisions` |
| `RepeatedPlayPauseTogglesReturnToTheOriginalState` | port | `app` | M7b | `repeated_play_pause_toggles_return_to_the_original_state` |
| `TransportPollsOnlyRepublishWhenPlaybackStateChanges` | port | `app` | M7b | `transport_republishes_only_when_playback_state_changes` |
| `QueueMutationStillPublishesWhenPreparingNextFails` | port | `app` | M7b | `queue_mutation_still_publishes_when_preparing_next_fails` |
| `ActivePlaylistMutationsNotifyThePlatformSink` | port | `app` | M7a | `active_playlist_mutations_notify_observers` |
| `SequenceHydrationRoutesOccurrencesAndPreservesIntentionalDuplicates` | port | `app` | M7a | `sequence_hydration_routes_occurrences_and_preserves_intentional_duplicates` |
| `SequenceRefillRestoresFrozenRoutesAndKeepsAPausedTailPaused` | port | `app` | M7a | `sequence_refill_restores_frozen_routes_and_keeps_a_paused_tail_paused` |
| `PartyCommandReceiptSurvivesRestartWithoutRepeatingNext` | port | `party` | M8 | `party_command_receipt_survives_restart_without_repeating_next` |
| `PartyNextSkipsRepeatedTrack` | port | `party` | M8 | `party_next_skips_repeated_track` |
| `PartyProjectionBoundsLargeQueuesAndRelaysOnlyPublicMetadata` | port | `party` | M8 | `party_projection_bounds_large_queues_and_relays_only_public_metadata` |
| `PartyProjectionPublishesRequestsOnceAndKeepsDuplicateTracksDistinct` | port | `party` | M8 | `party_projection_publishes_requests_once_and_keeps_duplicate_tracks_distinct` |
| `PartyProjectionRepeatWrapPrefersFutureOccurrencesOverHistory` | port | `party` | M8 | `party_projection_repeat_wrap_prefers_future_occurrences_over_history` |
| `PartyStartReplacesCollectionWithoutReplaying` | port | `party` | M8 | `party_start_replaces_collection_without_replaying` |
| `PartyRejectsTrackTheHostCannotResolve` | port | `party` | M8 | `party_rejects_track_the_host_cannot_resolve` |
| `PartyNextSkipsUnplayableTrackToNextThatOpens` | port | `party` | M8 | `party_next_skips_unplayable_track_to_next_that_opens` |
| `PartyPreviousSkipsUnplayableTrackBackwards` | port | `party` | M8 | `party_previous_skips_unplayable_track_backwards` |
| `PartyNextPastLastPlayableTrackDropsPreviousAudio` | port | `party` | M8 | `party_next_past_last_playable_track_drops_previous_audio` |
| `UnplayableTracksAreReportedAndSkippedOnStartAndNext` | port | `app` | M7b | `unplayable_tracks_are_reported_and_skipped_on_start_and_next` |
| `PreviousSkipsUnplayableTracksBackwards` | port | `app` | M7b | `previous_skips_unplayable_tracks_backwards` |
| `TransientFailureStopsOnTheTrackInsteadOfSkipping` | port | `app` | M7b | `transient_failure_stops_on_the_track_instead_of_skipping` |
| `PartySeekRequestIdIsPublishedWithTransport` | port | `party` | M8 | `party_seek_request_id_is_published_with_transport` |
| `PartyQueuePublishesPrivateSoundCloudTrackAsReferenceOnly` | port | `party` | M8 | `party_queue_publishes_private_track_as_reference_only` |
| `PartyGuestCannotStartLocalPlaybackAndCanResumeAfterLeaving` | port | `party` | M8 | `party_guest_cannot_start_local_playback_and_can_resume_after_leaving` |
| `PartyRejectsStaleRoomAndPersistsRejectedCommandOutcome` | port | `party` | M8 | `party_rejects_command_for_ended_party_and_persists_rejection` |
| `SequenceCatalogPersistsCacheAndInvalidationCannotResurrectAStaleWalk` | port | `app` | M7a | `sequence_catalog_persists_cache_and_invalidation_cannot_resurrect_a_stale_walk` |
| `SequenceCatalogNamespacesCompositeOccurrencesAndKeepsPositionalProvidersFirst` | port | `app` | M7a | `sequence_catalog_namespaces_composite_occurrences_and_keeps_positional_providers_first` |
| `PartyShuffleUpgradesTheFullLikedSequenceWithoutRestartingCurrentTrack` | port | `app` | M7a | `shuffle_covers_the_full_liked_sequence_without_restarting_current_track` |
| `DelayedSequenceStartCannotReplaceANewerPlaybackChoice` | port | `app` | M7a | `delayed_sequence_start_cannot_replace_a_newer_playback_choice` |
| `NativePartyConnectionIsLazyAndRetriesWithoutReexecutingCommands` | port | `party` | M8 | `party_connection_is_lazy_and_resends_without_reexecuting_commands` |
| `NativePartyExecutesTypedQueuePositionsWithoutJsonCoercion` | retire | | | Guarded JSON-as-ABI coercion between React and C++. Commands are typed `ristle-protocol` values; there is no JSON layer to coerce. |
| `NativePartyExpiryDisconnectsWithoutReactPolling` | port | `party` | M8 | `party_expiry_disconnects_without_ui_polling` |
| `NativePartyResumeClearsAnEndedRoomMarker` | port | `party` | M8 | `party_resume_clears_an_ended_party_marker` |
| `PartyLeaseKeySurvivesCoreRestartAndNeverReachesPresentation` | retire | | | Playback leases are dropped (ADR 0007). |
| `ExpiredPartyCredentialsAreRemovedOnRestartWithoutConnecting` | port | `party` | M8 | `expired_party_credentials_are_removed_on_restart_without_connecting` |
| `PagedQueueRefillsWithoutReactAndRejectsPreviousGeneration` | port | `app` | M7a | `paged_queue_refills_in_core_and_rejects_previous_generation` |
| `PagedQueueBackgroundRefillResumesDrainedTail` | port | `app` | M7a | `paged_queue_background_refill_resumes_drained_tail` |
| `NativeRefillNotifiesFailureAndRecoveryWithoutReactPolling` | port | `app` | M7a | `refill_publishes_failure_and_recovery` |

### `packages/app-core/tests/provider_queue_page_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `NativeLikedRefillPreservesSparseProviderAndChronologicalOrder` | port | `app` | M7a | `liked_refill_preserves_sparse_provider_and_chronological_order` |
| `NativeLikedRefillRetriesWholeCursorAfterProviderFailure` | port | `app` | M7a | `liked_refill_retries_whole_cursor_after_provider_failure` |
| `NativeRefillRejectsNonAdvancingProviderCursor` | port | `app` | M7a | `refill_rejects_non_advancing_provider_cursor` |

### `packages/app-core/tests/party_audio_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `PartyMp3SegmentsRetainDurationAndSignal` | phase 5 | | | File Share audio framing; built with the File Share transport in phase 5. |
| `PartyCacheBuffersEncryptedSegmentsAndRevokesOpenSources` | phase 5 | | | File Share receive cache; phase 5 with the transport. |
| `PartyPeersTransferMp3OverIceWithoutServerAudioStorage` | phase 5 | | | Needs ICE, TURN and a second peer; phase 5. |

### `packages/app-core/tests/party_file_policy_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `PartyFileExpiresAtTwoHoursWithoutRenewal` | port | `party` | M8 | `party_file_expires_at_two_hours_without_renewal` |
| `PartyFileExpiryAllowsOnlyTheAlreadyPlayingOccurrenceToFinish` | port | `party` | M8 | `party_file_expiry_allows_only_the_already_playing_occurrence_to_finish` |
| `PartyFileGuestDepartureRevokesPendingAndPreservesCurrentUntilEnd` | port | `party` | M8 | `party_file_guest_departure_revokes_pending_and_preserves_current_until_end` |
| `PartyFileQueueRemovalAndPartyEndReleaseAssets` | port | `party` | M8 | `party_file_queue_removal_and_party_end_release_assets` |
| `PartyRelayAdmissionReservesPlaybackHeadroom` | phase 5 | | | TURN bandwidth admission is the server's TURN broker (ADR 0005); ported into `server/` in phase 5. |

### `packages/app-core/tests/party_projection_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `PartyProjectionPreservesOccurrencesAndSampleReceiptTime` | port | `party` | M8 | `party_projection_preserves_occurrences_and_sample_receipt_time` |
| `PartyProjectionShowsRelayedMetadataWithoutProviderConnection` | port | `party` | M8 | `party_projection_shows_relayed_metadata_without_provider_connection` |
| `PartyProjectionDiscardsPreviousAccountMetadataAndDeduplicatesReferences` | port | `party` | M8 | `party_projection_discards_previous_account_metadata_and_deduplicates_references` |

### `packages/app-core/tests/party_search_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `PartySearchInterleavesPublicMetadataAndFencesReplyAfterReconnect` | port | `party` | M8 | `party_search_interleaves_public_metadata_and_fences_reply_after_reconnect` |
| `PartySearchDiscardsWorkForExpiredRequests` | port | `party` | M8 | `party_search_discards_work_for_expired_requests` |
| `PartyNumericCodecRejectsWrappingAndTruncation` | port | `protocol` | M8 | `numeric_fields_reject_wrapping_and_truncation` |
| `PartyTrackSerializationPreservesProviderMetadata` | port | `protocol` | M8 | `track_serialization_preserves_provider_metadata` |
| `PartySearchTrimsLargeMetadataRepliesToTransportBudget` | port | `party` | M8 | `party_search_trims_large_metadata_replies_to_transport_budget` |
| `PartyPublicProjectionKeepsOnlyCatalogFields` | port | `party` | M8 | `party_projection_keeps_only_shared_fields` |
| `PartyQueueBudgetPreservesWorstCaseWindowAndCurrentMetadata` | port | `party` | M8 | `party_queue_budget_preserves_worst_case_window_and_current_metadata` |
| `PartyPublicProjectionHidesPrivateSoundCloudTracks` | port | `party` | M8 | `party_projection_hides_private_tracks` |
| `PartyGuestSearchUsesLocalTrackAndFallsBackToRelay` | port | `party` | M8 | `party_guest_search_uses_local_track_and_falls_back_to_relay` |
| `PartyRelaysWhyTheHostCannotPlayATrack` | port | `party` | M8 | `party_relays_why_the_host_cannot_play_a_track` |

### `packages/app-core/tests/party_transport_smoke.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `party_transport_smoke` | retire | | | Manual executable against a live SpacetimeDB room, not a test case. ADR 0007 replaced SpacetimeDB. |

### `packages/app-core/tests/party_view_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `PartyViewOwnsExpiryMembershipAndHostFencing` | port | `party` | M8 | `party_view_owns_expiry_membership_and_host_fencing` |
| `PartyViewSelectsLiveMembershipBeforeAnEndedRoom` | port | `party` | M8 | `party_view_selects_live_membership_before_an_ended_party` |

### `packages/audio-engine/tests/audio_engine_test.cpp`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `AudioEngineJoinsPreparedSourcesAtTheExactFrameBoundary` | port | `audio` | M6 | `audio_engine_joins_prepared_sources_at_the_exact_frame_boundary` |
| `AudioEngineSeekSequencePreventsStalePositionAuthority` | port | `audio` | M6 | `audio_engine_seek_sequence_prevents_stale_position_authority` |
| `AudioEnginePausedTrackStopsBufferingOnceDecoded` | port | `audio` | M6 | `audio_engine_paused_track_stops_buffering_once_decoded` |
| `AudioEngineAppliesOneCoreOwnedGain` | port | `audio` | M6 | `audio_engine_applies_one_core_owned_gain` |
| `AudioEngineCrossfadesIntoPreparedSource` | retire | | | No crossfade in Ristle (ADR 0012). A later crossfade feature brings its own tests, honouring `overlap = false`. |
| `AudioEnginePromotesBufferedNextWithoutReopeningIt` | port | `audio` | M6 | `audio_engine_promotes_buffered_next_without_reopening_it` |
| `AudioEngineDoesNotPromoteFailedPreparedNext` | port | `audio` | M6 | `audio_engine_does_not_promote_failed_prepared_next` |
| `AudioEngineCrossfadesAfterSeekingIntoLongFadeWindow` | retire | | | No crossfade (ADR 0012). |
| `AudioEnginePrebuffersTheEntireMaximumCrossfade` | retire | | | No crossfade (ADR 0012). |
| `AudioEngineKeepsPollingSourcesThatAreNotExhausted` | port | `audio` | M6 | `audio_engine_keeps_polling_sources_that_are_not_exhausted` |
| `AudioEngineRetainsSourceFailuresForDiagnostics` | port | `audio` | M6 | `audio_engine_retains_source_failures_for_diagnostics` |

### `packages/party-client/tests/events.rs`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `typed_delivery_orders_transaction_changes_before_acceptance` | retire | | | SpacetimeDB transaction delivery. ADR 0007 puts outcomes inside Party State, so no ordering between them exists. |
| `reconnect_changes_generation_and_close_wakes_outcome_waiters` | retire | | | SpacetimeDB client generations and outcome waiters, replaced by resend-above-ack (ADR 0007). |
| `committed_write_with_lost_ack_is_indeterminate_and_recovers_on_reset` | retire | | | Indeterminate writes are resolved by resending, idempotent by construction (ADR 0007); covered by `party_connection_is_lazy_and_resends_without_reexecuting_commands`. |

### `packages/party-client/src/delivery.rs`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `overflow_preserves_results_and_releases_capacity_only_on_consumption` | retire | | | Outcome delivery queue of the SpacetimeDB client. Outcomes are read from Party State (ADR 0007). |
| `reconnect_retains_the_origin_of_indeterminate_results` | retire | | | Indeterminate results do not exist under resend-above-ack (ADR 0007). |
| `repeated_connection_failure_does_not_grow_the_queue` | retire | | | Outcome delivery queue, gone with ADR 0007. The guest's bounded pending-command list is covered by `party_connection_is_lazy_and_resends_without_reexecuting_commands`. |
| `consumed_operation_ids_cannot_be_reused_by_late_callbacks` | retire | | | Operation ids and late SpacetimeDB callbacks, replaced by per-member sequence numbers (ADR 0007). |

### `packages/party-client/src/ffi.rs`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `lease_key_is_random_and_restores_the_same_public_key` | retire | | | Playback leases are dropped (ADR 0007). |
| `invite_secret_is_not_derived_from_the_public_room_id` | port | `party` | M8 | `invite_secret_is_not_derived_from_the_public_party_id` |

### `packages/party-client/tests/maincloud.rs`

| Legacy test | Status | Crate | Milestone | Rust test, or reason |
|---|---|---|---|---|
| `maincloud_native_identity_rows_fencing_and_outcomes` | retire | | | Runs against SpacetimeDB maincloud, replaced by the Relay (ADR 0007). |


## 9. Decisions on the open questions (2026-10-19)

1. **Party core in phase 2.** `ristle-protocol` and `ristle-party` are built in
   M8 against an in-memory Relay, as recommended. The logic is core, and the
   40 legacy tests need no server.
2. **Four rows stay open past phase 2.** File Share audio framing, its receive
   cache, the ICE transfer and TURN admission are marked `phase 5`. Phase 2
   exits with them open, not retired.
3. **No import from Ristle legacy (user decision).** Ristle starts with a new
   database and imports nothing: no Ristle Playlists, no Recently Played, no
   settings. Nothing of value is lost. The two migration tests stay retired.
