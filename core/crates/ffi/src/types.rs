//! The records and enums of the facade.
//!
//! They are `ristle-app`'s own types, declared here as UniFFI remote types:
//! each declaration restates the type so UniFFI can generate its converter,
//! and the compiler rejects one that no longer matches. Field documentation
//! lives on the types in `ristle_app::model`.

pub use ristle_app::{
    AlbumView, ArtworkImage, Attribute, AttributeValue, Availability, CaptureRule, CaptureSource,
    ChoiceOption, CollectionRef, ConnectionState, ConnectionView, Consent, CoreConfig,
    CoreSnapshot, Duplicates, FieldKind, FolderView, FormField, InstallKind, InstallReview, ItemId,
    ItemRef, ItemView, LibraryChange, LibraryKind, LibraryView, LinkView, ListEntry, ListingPage,
    MediaCommand, MediaDate, MediaServer, More, Named, NamedValue, Opener, Page, PageScript,
    PageTarget, Part, Permission, PickedFolder, Placement, PlaySource, PlaybackNotice,
    PlaybackState, PlaylistAddOutcome, PlaylistDestination, PlaylistSummary, PlaylistView,
    PluginView, ProfilePage, ProfileView, ProviderErrorKind, QueueEntryView, QueueSource,
    QueueWindow, ReleaseType, RepeatMode, SearchGroup, SearchKind, SearchScope, SearchSnapshot,
    SectionLayout, Setting, SettingControl, SettingScope, SettingValue, SettingsGroup, Shelf,
    SignInAnswer, SignInForm, SignInStep, StartAt, TrackView, Transport, UserAgent, ViewPage,
    ViewRef, ViewSection, WebCapture,
};

// ristle_app::model::items

/// A Provider item: the Provider, its id there and, where the id is only
/// meaningful on one server (a Server Grant), the Provider Connection.
///
/// For a Track this is Track identity (`CONTEXT.md`, Track). Provider-owned
/// collections (a Provider Playlist, Liked Songs) also name their
/// connection.
#[uniffi::remote(Record)]
pub struct ItemId {
    pub provider: String,
    pub id: String,
    pub connection: Option<String>,
}

/// Something a user can play, queue, like or add to a playlist.
#[uniffi::remote(Enum)]
pub enum ItemRef {
    Track { item: ItemId },
    Album { item: ItemId },
    Artist { item: ItemId },
    User { item: ItemId },
    Playlist { item: ItemId },
    RistlePlaylist { id: String },
}

/// An artist, user or album named on another item, with its id when the
/// Provider has one.
#[uniffi::remote(Record)]
pub struct Named {
    pub id: Option<String>,
    pub name: String,
}

/// One rendition of artwork. The core has checked the URL against the
/// Plugin's image hosts; dimensions are hints for choosing a rendition.
#[uniffi::remote(Record)]
pub struct ArtworkImage {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// A Media Date, with the precision the Provider gave.
#[uniffi::remote(Record)]
pub struct MediaDate {
    pub year: u16,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

/// Whether a Track can play.
#[uniffi::remote(Enum)]
pub enum Availability {
    Available,
    Unavailable { reason: Option<String> },
    Unknown,
}

/// The value of a Metadata Attribute.
#[uniffi::remote(Enum)]
pub enum AttributeValue {
    Text { value: String },
    Number { value: f64 },
    YesNo { value: bool },
}

/// A Metadata Attribute: an optional fact a UI may show.
#[uniffi::remote(Record)]
pub struct Attribute {
    pub key: String,
    pub value: AttributeValue,
}

/// A Track as UIs show it.
#[uniffi::remote(Record)]
pub struct TrackView {
    pub item: ItemId,
    pub title: String,
    pub artists: Vec<Named>,
    pub album: Option<Named>,
    pub duration_ms: Option<u64>,
    pub artwork: Vec<ArtworkImage>,
    pub explicit: bool,
    pub live: bool,
    pub seekable: bool,
    pub availability: Availability,
    pub released: Option<MediaDate>,
    pub uploaded: Option<MediaDate>,
    pub url: Option<String>,
    pub attributes: Vec<Attribute>,
}

/// The kind of release an album is.
#[uniffi::remote(Enum)]
pub enum ReleaseType {
    Album,
    Single,
    Ep,
    Compilation,
}

/// An album as UIs show it.
#[uniffi::remote(Record)]
pub struct AlbumView {
    pub item: ItemId,
    pub title: String,
    pub artists: Vec<Named>,
    pub artwork: Vec<ArtworkImage>,
    pub released: Option<MediaDate>,
    pub release_type: Option<ReleaseType>,
    pub track_count: Option<u32>,
    pub url: Option<String>,
}

/// An artist or a user.
#[uniffi::remote(Record)]
pub struct ProfileView {
    pub item: ItemId,
    pub name: String,
    pub artwork: Vec<ArtworkImage>,
    pub about: Option<String>,
    pub followers: Option<u64>,
    pub location: Option<String>,
    pub url: Option<String>,
}

/// A Provider Playlist.
#[uniffi::remote(Record)]
pub struct PlaylistView {
    pub item: ItemId,
    pub title: String,
    pub description: Option<String>,
    pub owner: Option<Named>,
    pub artwork: Vec<ArtworkImage>,
    pub track_count: Option<u32>,
    pub created: Option<MediaDate>,
    pub updated: Option<MediaDate>,
    pub public: Option<bool>,
    pub editable: bool,
    pub url: Option<String>,
}

/// A row that opens a Provider View.
#[uniffi::remote(Record)]
pub struct LinkView {
    pub target: ViewRef,
    pub title: String,
    pub subtitle: Option<String>,
    pub artwork: Vec<ArtworkImage>,
}

/// A Provider View of one Provider Connection.
#[uniffi::remote(Record)]
pub struct ViewRef {
    pub connection: String,
    pub view: String,
    pub params: String,
}

/// A Provider-owned playlist folder. Not playable.
#[uniffi::remote(Record)]
pub struct FolderView {
    pub id: String,
    pub title: String,
    pub parent: Option<String>,
}

/// Anything a list can show.
#[uniffi::remote(Enum)]
pub enum ItemView {
    Track { track: TrackView },
    Album { album: AlbumView },
    Artist { profile: ProfileView },
    User { profile: ProfileView },
    Playlist { playlist: PlaylistView },
    Folder { folder: FolderView },
    Link { link: LinkView },
    Unavailable { reason: Option<String> },
}

// ristle_app::model::pages

/// How a Provider collection is read: the Provider Connection and the
/// Plugin's collection id, such as `playlist:42`.
///
/// This is the routing reference, kept apart from the entity the user
/// opened (legacy trap 5).
#[uniffi::remote(Record)]
pub struct CollectionRef {
    pub connection: String,
    pub id: String,
}

/// One row of a list.
#[uniffi::remote(Record)]
pub struct ListEntry {
    pub item: ItemView,
    pub added_at_ms: Option<u64>,
}

/// The first entries of a list, and how to read the rest.
#[uniffi::remote(Record)]
pub struct Part {
    pub entries: Vec<ListEntry>,
    pub more: Option<More>,
}

/// Where a [`Part`] continues.
#[uniffi::remote(Record)]
pub struct More {
    pub collection: CollectionRef,
    pub cursor: Option<String>,
}

/// One page of a collection, from `list`.
#[uniffi::remote(Record)]
pub struct ListingPage {
    pub entries: Vec<ListEntry>,
    pub cursor: Option<String>,
    pub total: Option<u64>,
}

/// Something a user can open as a page.
#[uniffi::remote(Enum)]
pub enum PageTarget {
    Album { item: ItemId },
    Artist { item: ItemId },
    User { item: ItemId },
    Playlist { item: ItemId },
    RistlePlaylist { id: String },
    View { view: ViewRef },
}

/// An artist's or a user's page. A part the Provider does not have is
/// `None` and is not shown.
#[uniffi::remote(Record)]
pub struct ProfilePage {
    pub profile: ProfileView,
    pub top_tracks: Option<Part>,
    pub releases: Option<Part>,
    pub tracks: Option<Part>,
    pub playlists: Option<Part>,
    pub appears_on: Option<Part>,
    pub likes: Option<Part>,
    pub reposts: Option<Part>,
    pub related: Option<Part>,
    pub discography: Option<CollectionRef>,
    pub extras: Vec<LinkView>,
}

/// How a section of a Provider View is laid out.
#[uniffi::remote(Enum)]
pub enum SectionLayout {
    Rows,
    Shelf,
    Grid,
}

/// One block of a Provider View.
#[uniffi::remote(Record)]
pub struct ViewSection {
    pub title: Option<String>,
    pub layout: SectionLayout,
    pub content: Part,
}

/// A Provider View, laid out with the standard list UI.
#[uniffi::remote(Record)]
pub struct ViewPage {
    pub title: String,
    pub subtitle: Option<String>,
    pub artwork: Vec<ArtworkImage>,
    pub sections: Vec<ViewSection>,
}

/// A page, as data. UIs own one layout per kind.
#[uniffi::remote(Enum)]
pub enum Page {
    Album {
        album: AlbumView,
        tracks: Part,
    },
    Playlist {
        playlist: PlaylistView,
        tracks: Part,
    },
    RistlePlaylist {
        playlist: PlaylistSummary,
        tracks: Part,
    },
    Artist {
        page: ProfilePage,
    },
    User {
        page: ProfilePage,
    },
    View {
        page: ViewPage,
    },
}

/// A standard part of a user's library on one Provider.
#[uniffi::remote(Enum)]
pub enum LibraryKind {
    LikedTracks,
    SavedAlbums,
    FollowedArtists,
    FollowedUsers,
    Playlists,
    Uploads,
}

/// One shelf of the library.
#[uniffi::remote(Record)]
pub struct Shelf {
    pub kind: LibraryKind,
    pub title: String,
    pub provider: String,
    pub collection: CollectionRef,
}

/// The user's library across every Provider Connection.
#[uniffi::remote(Record)]
pub struct LibraryView {
    pub shelves: Vec<Shelf>,
    pub views: Vec<LinkView>,
}

// ristle_app::model::playlists

/// A Ristle Playlist in a list of playlists.
#[uniffi::remote(Record)]
pub struct PlaylistSummary {
    pub id: String,
    pub title: String,
    pub track_count: u64,
    pub updated_at_ms: u64,
}

/// Where a Playlist Add goes.
#[uniffi::remote(Enum)]
pub enum PlaylistDestination {
    Ristle { id: String },
    NewRistle { title: String },
    Provider { item: ItemId },
}

/// What to do with Tracks the destination already has.
#[uniffi::remote(Enum)]
pub enum Duplicates {
    Ask,
    AddAll,
    SkipExisting,
}

/// The result of a Playlist Add.
#[uniffi::remote(Enum)]
pub enum PlaylistAddOutcome {
    Added {
        playlist: String,
        added: u64,
        skipped: u64,
    },
    DuplicatesFound {
        duplicates: u64,
        total: u64,
    },
}

// ristle_app::model::search

/// Which catalogs a search covers.
#[uniffi::remote(Enum)]
pub enum SearchScope {
    Everywhere,
    MyLibrary,
}

/// The kind of results in a group.
#[uniffi::remote(Enum)]
pub enum SearchKind {
    Tracks,
    Albums,
    Artists,
    Users,
    Playlists,
}

/// The results of one kind, with Providers interleaved by relevance band.
#[uniffi::remote(Record)]
pub struct SearchGroup {
    pub kind: SearchKind,
    pub items: Vec<ItemView>,
}

/// One stable snapshot of a Unified Search: taken once every Provider
/// answered or five seconds passed. Late answers never reorder it.
#[uniffi::remote(Record)]
pub struct SearchSnapshot {
    pub id: u64,
    pub query: String,
    pub groups: Vec<SearchGroup>,
    pub missing: Vec<String>,
}

// ristle_app::model::session

/// Where the core keeps its files.
#[uniffi::remote(Record)]
pub struct CoreConfig {
    pub data_dir: String,
    pub cache_dir: String,
}

/// What the player is doing.
#[uniffi::remote(Enum)]
pub enum PlaybackState {
    Idle,
    Buffering,
    Playing,
    Paused,
}

/// Repeat state.
#[uniffi::remote(Enum)]
pub enum RepeatMode {
    Off,
    Track,
    Queue,
}

/// The transport: state and playhead.
#[uniffi::remote(Record)]
pub struct Transport {
    pub state: PlaybackState,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub seekable: bool,
    pub volume: f32,
}

/// One entry of the Canonical Queue.
#[uniffi::remote(Record)]
pub struct QueueEntryView {
    pub entry: u64,
    pub track: TrackView,
    pub submitter: Option<String>,
}

/// Where the queue's continuation comes from, for display and navigation.
#[uniffi::remote(Record)]
pub struct QueueSource {
    pub title: String,
    pub opened: Option<ItemRef>,
}

/// The part of the Canonical Queue UIs show. The core decides how much
/// history and lookahead fit; the rest of a long collection is not
/// materialized.
#[uniffi::remote(Record)]
pub struct QueueWindow {
    pub history: Vec<QueueEntryView>,
    pub current: Option<QueueEntryView>,
    pub requests: Vec<QueueEntryView>,
    pub upcoming: Vec<QueueEntryView>,
    pub upcoming_total: Option<u64>,
    pub source: Option<QueueSource>,
}

/// Everything a now-playing screen, a queue screen and a platform media
/// session render. Published whole, with a revision that only grows.
#[uniffi::remote(Record)]
pub struct CoreSnapshot {
    pub revision: u64,
    pub transport: Transport,
    pub queue: QueueWindow,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}

/// Where `add_to_queue` puts items.
#[uniffi::remote(Enum)]
pub enum Placement {
    Next,
    Last,
}

/// What to start playing.
#[uniffi::remote(Enum)]
pub enum PlaySource {
    Items { items: Vec<ItemRef> },
    Collection { collection: CollectionRef },
    LikedTracks,
    SearchResults { search: u64 },
}

/// Where in a [`PlaySource`] to start.
#[uniffi::remote(Enum)]
pub enum StartAt {
    Beginning,
    Position { position: u64 },
}

/// A command from a platform media session: lock screen, notification,
/// headset, Android Auto or CarPlay.
#[uniffi::remote(Enum)]
pub enum MediaCommand {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    SeekTo { position_ms: u64 },
    SeekBy { offset_ms: i64 },
    SetRepeat { mode: RepeatMode },
    SetShuffle { enabled: bool },
    SetCurrentLiked { liked: bool },
    JumpTo { entry: u64 },
}

/// A message above the seek bar when playback is delayed or interrupted.
/// Never a dialog; the transport stays usable.
#[uniffi::remote(Enum)]
pub enum PlaybackNotice {
    Delayed {
        track: TrackView,
    },
    Skipped {
        track: TrackView,
        reason: Option<String>,
    },
    Stopped {
        track: TrackView,
        reason: Option<String>,
    },
    SignInNeeded {
        connection: String,
    },
    Cleared,
}

/// Something outside the Playback Session changed; UIs reload what shows it.
#[uniffi::remote(Enum)]
pub enum LibraryChange {
    RistlePlaylists,
    Liked { item: ItemRef, liked: bool },
    RecentlyPlayed,
    Plugins,
    LibraryView { connection: String },
}

// ristle_app::model::plugins

/// The state of a Provider Connection.
#[uniffi::remote(Enum)]
pub enum ConnectionState {
    Ready,
    NeedsSignIn,
    Unavailable { reason: String },
}

/// A Provider Connection.
#[uniffi::remote(Record)]
pub struct ConnectionView {
    pub id: String,
    pub plugin: String,
    pub name: String,
    pub avatar: Vec<ArtworkImage>,
    pub state: ConnectionState,
}

/// An installed Plugin.
#[uniffi::remote(Record)]
pub struct PluginView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub enabled: bool,
    pub has_accounts: bool,
    pub connections: Vec<ConnectionView>,
    pub update: Option<String>,
}

/// What installing a reviewed package would do.
#[uniffi::remote(Enum)]
pub enum InstallKind {
    New,
    Update { from_version: String },
    Downgrade { from_version: String },
    Takeover { from_publisher: String },
}

/// One line of the consent screen. UIs word them (packaging.md,
/// "Permissions and consent").
#[uniffi::remote(Enum)]
pub enum Permission {
    ConnectsTo { hosts: Vec<String> },
    ChecksForUpdates { host: String },
    SharesWithPartyGuests { fields: Vec<String> },
    StoresUpTo { mib: u32 },
    ChosenServers,
    ChosenFolders,
    OpensSockets { targets: Vec<String> },
    ObservesSignInSites { hosts: Vec<String> },
    OpensApps { apps: Vec<String> },
    SignsIn,
    Likes,
    Reposts,
    EditsPlaylists,
    ProjectsQueue,
}

/// A reviewed Plugin Package, ready to install with the user's consent.
#[uniffi::remote(Record)]
pub struct InstallReview {
    pub review: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub kind: InstallKind,
    pub permissions: Vec<Permission>,
    pub icon: Option<Vec<u8>>,
}

/// The user's answer to an install review. It must match the review's
/// [`InstallKind`]: a takeover or downgrade is never accepted by `Install`.
#[uniffi::remote(Enum)]
pub enum Consent {
    Install,
    Downgrade,
    Takeover,
}

/// Who a Plugin setting applies to.
#[uniffi::remote(Enum)]
pub enum SettingScope {
    Plugin,
    Connection,
}

/// One option of a choice.
#[uniffi::remote(Record)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
}

/// The control of one setting, with its current value.
#[uniffi::remote(Enum)]
pub enum SettingControl {
    Toggle {
        value: bool,
    },
    Choice {
        options: Vec<ChoiceOption>,
        value: String,
    },
    Text {
        secret: bool,
        placeholder: Option<String>,
        value: String,
    },
    Number {
        min: f64,
        max: f64,
        step: f64,
        unit: Option<String>,
        value: f64,
    },
    Button {
        confirm: Option<String>,
    },
}

/// One Plugin setting.
#[uniffi::remote(Record)]
pub struct Setting {
    pub key: String,
    pub title: String,
    pub description: Option<String>,
    pub scope: SettingScope,
    pub control: SettingControl,
}

/// A group of Plugin settings.
#[uniffi::remote(Record)]
pub struct SettingsGroup {
    pub title: Option<String>,
    pub settings: Vec<Setting>,
}

/// A new value for a Plugin setting.
#[uniffi::remote(Enum)]
pub enum SettingValue {
    Toggle { value: bool },
    Choice { value: String },
    Text { value: String },
    Number { value: f64 },
    Pressed,
}

// ristle_app::model::sign_in

/// How a sign-in URL is opened.
#[uniffi::remote(Enum)]
pub enum Opener {
    Browser,
    App,
}

/// The kind of a form field.
#[uniffi::remote(Enum)]
pub enum FieldKind {
    Text,
    Secret,
    Email,
    Url,
}

/// One field of a sign-in form.
#[uniffi::remote(Record)]
pub struct FormField {
    pub name: String,
    pub label: String,
    pub kind: FieldKind,
    pub required: bool,
    pub placeholder: Option<String>,
    pub value: Option<String>,
}

/// A sign-in form.
#[uniffi::remote(Record)]
pub struct SignInForm {
    pub title: String,
    pub instructions: Option<String>,
    pub fields: Vec<FormField>,
    pub help_url: Option<String>,
}

/// The user agent of a sign-in web view.
#[uniffi::remote(Enum)]
pub enum UserAgent {
    Platform,
    Desktop,
    Custom { value: String },
}

/// Where a captured value is read in a sign-in web view.
#[uniffi::remote(Enum)]
pub enum CaptureSource {
    Cookie { name: String },
    RequestHeader { name: String },
    ResponseHeader { name: String },
    QueryParam { url_prefix: String, name: String },
    Script,
}

/// One value a sign-in web view captures.
#[uniffi::remote(Record)]
pub struct CaptureRule {
    pub name: String,
    pub sources: Vec<CaptureSource>,
    pub required: bool,
}

/// A script injected into pages whose URL starts with a prefix.
#[uniffi::remote(Record)]
pub struct PageScript {
    pub url_prefix: String,
    pub code: String,
}

/// A sign-in web view with capture rules. The core has checked every URL
/// and host against the Plugin's manifest.
#[uniffi::remote(Record)]
pub struct WebCapture {
    pub start_url: String,
    pub user_agent: UserAgent,
    pub observe: Vec<String>,
    pub captures: Vec<CaptureRule>,
    pub scripts: Vec<PageScript>,
}

/// A self-hosted server, such as Jellyfin.
#[uniffi::remote(Record)]
pub struct MediaServer {
    pub name: String,
    pub urls: Vec<String>,
}

/// A folder the user picked with the system folder picker.
#[uniffi::remote(Record)]
pub struct PickedFolder {
    pub uri: String,
    pub name: String,
}

/// A name and a value from a form or a web capture.
#[uniffi::remote(Record)]
pub struct NamedValue {
    pub name: String,
    pub value: String,
}

/// The next thing a sign-in needs from the user.
#[uniffi::remote(Enum)]
pub enum SignInStep {
    OpenUrl {
        session: String,
        url: String,
        opener: Opener,
        fallback_url: Option<String>,
    },
    Form {
        session: String,
        form: SignInForm,
    },
    WebCapture {
        session: String,
        capture: WebCapture,
    },
    DeviceCode {
        session: String,
        verification_url: String,
        user_code: String,
        expires_at_ms: u64,
    },
    ChooseServer {
        session: String,
        found: Vec<MediaServer>,
        allow_entry: bool,
        placeholder: Option<String>,
    },
    ChooseFolders {
        session: String,
        prompt: Option<String>,
        multiple: bool,
    },
    Done {
        connection: ConnectionView,
    },
}

/// The user's answer to a [`SignInStep`].
#[uniffi::remote(Enum)]
pub enum SignInAnswer {
    Callback { url: String },
    Form { values: Vec<NamedValue> },
    WebCapture { values: Vec<NamedValue> },
    Continue,
    Server { server: MediaServer },
    Folders { folders: Vec<PickedFolder> },
}

// ristle_app (errors)

/// What kind of Provider failure it was. `Unplayable`, `Account` and
/// `Unknown` are reported and skipped; `Transient` stops on the Track,
/// paused.
#[uniffi::remote(Enum)]
pub enum ProviderErrorKind {
    Unplayable,
    Account,
    Transient,
    Unknown,
}
