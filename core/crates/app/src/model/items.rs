//! What lists, pages and the queue show: Tracks, albums, profiles, playlists
//! and links, and the references used to act on them.

/// A Provider item: the Provider, its id there and, where the id is only
/// meaningful on one server (a Server Grant), the Provider Connection.
///
/// For a Track this is Track identity (`CONTEXT.md`, Track). Provider-owned
/// collections (a Provider Playlist, Liked Songs) also name their
/// connection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemId {
    /// The Provider Plugin's manifest id, such as `spotify`.
    pub provider: String,
    /// The Provider's own id.
    pub id: String,
    /// The Provider Connection the id belongs to, when it is not a catalog id.
    pub connection: Option<String>,
}

/// Something a user can play, queue, like or add to a playlist.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ItemRef {
    /// One Track.
    Track {
        /// The Track.
        item: ItemId,
    },
    /// An album, in album order.
    Album {
        /// The album.
        item: ItemId,
    },
    /// An artist: their discography when played, a follow when liked.
    Artist {
        /// The artist.
        item: ItemId,
    },
    /// A user: a follow when liked.
    User {
        /// The user.
        item: ItemId,
    },
    /// A Provider Playlist.
    Playlist {
        /// The playlist.
        item: ItemId,
    },
    /// A Ristle Playlist.
    RistlePlaylist {
        /// The playlist's id.
        id: String,
    },
}

/// An artist, user or album named on another item, with its id when the
/// Provider has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The Provider's id, which makes the name a link.
    pub id: Option<String>,
    /// The name as the Provider shows it.
    pub name: String,
}

/// One rendition of artwork. The core has checked the URL against the
/// Plugin's image hosts; dimensions are hints for choosing a rendition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkImage {
    /// HTTPS URL, fetched through the core's image loader.
    pub url: String,
    /// Width in pixels, when the Provider says.
    pub width: Option<u32>,
    /// Height in pixels, when the Provider says.
    pub height: Option<u32>,
}

/// A Media Date, with the precision the Provider gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaDate {
    /// The year.
    pub year: u16,
    /// The month, 1 to 12, when known.
    pub month: Option<u8>,
    /// The day of the month, when known. Only set together with `month`.
    pub day: Option<u8>,
}

/// Whether a Track can play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// It can play.
    Available,
    /// A Disabled Track: listed with its metadata, but never queued.
    Unavailable {
        /// Why, as the Provider or the core put it.
        reason: Option<String>,
    },
    /// Not known yet, such as a Track restored from storage whose Plugin has
    /// not answered since.
    Unknown,
}

/// The value of a Metadata Attribute.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    /// Free text.
    Text {
        /// The text.
        value: String,
    },
    /// A number, such as beats per minute.
    Number {
        /// The number.
        value: f64,
    },
    /// Yes or no.
    YesNo {
        /// The answer.
        value: bool,
    },
}

/// A Metadata Attribute: an optional fact a UI may show.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// `genre`, `bpm`, `musical-key`, `mood`, `label`, `copyright`, or a key
    /// prefixed with the Provider id.
    pub key: String,
    /// The value.
    pub value: AttributeValue,
}

/// A Track as UIs show it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackView {
    /// Track identity.
    pub item: ItemId,
    /// The title.
    pub title: String,
    /// The artists, in the Provider's order.
    pub artists: Vec<Named>,
    /// The album, when the Track has one.
    pub album: Option<Named>,
    /// The duration. `None` for a Live Track or when the Provider does not say.
    pub duration_ms: Option<u64>,
    /// Artwork renditions.
    pub artwork: Vec<ArtworkImage>,
    /// Marked explicit by the Provider.
    pub explicit: bool,
    /// A Live Track: unknown or advancing end.
    pub live: bool,
    /// The playhead can be moved.
    pub seekable: bool,
    /// Whether it can play.
    pub availability: Availability,
    /// The release date.
    pub released: Option<MediaDate>,
    /// The upload date, for Providers where users upload.
    pub uploaded: Option<MediaDate>,
    /// The Track's page on the Provider ("Open on ..."). HTTPS only.
    pub url: Option<String>,
    /// Metadata Attributes.
    pub attributes: Vec<Attribute>,
}

/// The kind of release an album is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseType {
    /// An album.
    Album,
    /// A single.
    Single,
    /// An EP.
    Ep,
    /// A compilation.
    Compilation,
}

/// An album as UIs show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumView {
    /// The album.
    pub item: ItemId,
    /// The title.
    pub title: String,
    /// The artists.
    pub artists: Vec<Named>,
    /// Artwork renditions.
    pub artwork: Vec<ArtworkImage>,
    /// The release date.
    pub released: Option<MediaDate>,
    /// The kind of release.
    pub release_type: Option<ReleaseType>,
    /// The number of Tracks.
    pub track_count: Option<u32>,
    /// The album's page on the Provider. HTTPS only.
    pub url: Option<String>,
}

/// An artist or a user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileView {
    /// The artist or user.
    pub item: ItemId,
    /// The name.
    pub name: String,
    /// Artwork renditions.
    pub artwork: Vec<ArtworkImage>,
    /// Biography or profile text.
    pub about: Option<String>,
    /// Follower count.
    pub followers: Option<u64>,
    /// City or country, as the Provider shows it.
    pub location: Option<String>,
    /// The profile's page on the Provider. HTTPS only.
    pub url: Option<String>,
}

/// A Provider Playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistView {
    /// The playlist.
    pub item: ItemId,
    /// The title.
    pub title: String,
    /// The description.
    pub description: Option<String>,
    /// The owner.
    pub owner: Option<Named>,
    /// Artwork renditions.
    pub artwork: Vec<ArtworkImage>,
    /// The number of Tracks.
    pub track_count: Option<u32>,
    /// When it was created.
    pub created: Option<MediaDate>,
    /// When it last changed.
    pub updated: Option<MediaDate>,
    /// Whether it is public on the Provider, when the Provider says.
    pub public: Option<bool>,
    /// The connected account may change it.
    pub editable: bool,
    /// The playlist's page on the Provider. HTTPS only.
    pub url: Option<String>,
}

/// A row that opens a Provider View.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkView {
    /// The view it opens.
    pub target: ViewRef,
    /// The title.
    pub title: String,
    /// A second line.
    pub subtitle: Option<String>,
    /// Artwork renditions.
    pub artwork: Vec<ArtworkImage>,
}

/// A Provider View of one Provider Connection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewRef {
    /// The Provider Connection whose Plugin draws the view.
    pub connection: String,
    /// The Plugin's name for the view.
    pub view: String,
    /// Opaque parameters, passed back unchanged.
    pub params: String,
}

/// A Provider-owned playlist folder. Not playable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderView {
    /// The Provider's id.
    pub id: String,
    /// The title.
    pub title: String,
    /// The folder that holds this one.
    pub parent: Option<String>,
}

/// Anything a list can show.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemView {
    /// A Track.
    Track {
        /// The Track.
        track: TrackView,
    },
    /// An album.
    Album {
        /// The album.
        album: AlbumView,
    },
    /// An artist.
    Artist {
        /// The artist.
        profile: ProfileView,
    },
    /// A user.
    User {
        /// The user.
        profile: ProfileView,
    },
    /// A Provider Playlist.
    Playlist {
        /// The playlist.
        playlist: PlaylistView,
    },
    /// A playlist folder.
    Folder {
        /// The folder.
        folder: FolderView,
    },
    /// A link to a Provider View.
    Link {
        /// The link.
        link: LinkView,
    },
    /// A position the Provider holds but cannot describe, such as a deleted
    /// Track in a playlist.
    Unavailable {
        /// Why, when known.
        reason: Option<String>,
    },
}
