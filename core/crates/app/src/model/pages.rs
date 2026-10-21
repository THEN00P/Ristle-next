//! Pages, collections and the library.

use super::items::{
    AlbumView, ArtworkImage, ItemId, ItemView, LinkView, PlaylistView, ProfileView, ViewRef,
};
use super::playlists::PlaylistSummary;

/// How a Provider collection is read: the Provider Connection and the
/// Plugin's collection id, such as `playlist:42`.
///
/// This is the routing reference, kept apart from the entity the user
/// opened (legacy trap 5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CollectionRef {
    /// The Provider Connection.
    pub connection: String,
    /// The Plugin's collection id.
    pub id: String,
}

/// One row of a list.
#[derive(Debug, Clone, PartialEq)]
pub struct ListEntry {
    /// What the row shows.
    pub item: ItemView,
    /// When it was added to this list (the like date in Liked Songs), in
    /// milliseconds since the Unix epoch.
    pub added_at_ms: Option<u64>,
}

/// The first entries of a list, and how to read the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The entries shown inline.
    pub entries: Vec<ListEntry>,
    /// `None` when `entries` is the whole list.
    pub more: Option<More>,
}

/// Where a [`Part`] continues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct More {
    /// The collection to page through with `list`.
    pub collection: CollectionRef,
    /// The cursor to pass to `list` for the next entries.
    pub cursor: Option<String>,
}

/// One page of a collection, from `list`.
#[derive(Debug, Clone, PartialEq)]
pub struct ListingPage {
    /// The entries.
    pub entries: Vec<ListEntry>,
    /// The cursor for the following page; `None` at the end.
    pub cursor: Option<String>,
    /// The number of entries in the whole collection, when known.
    pub total: Option<u64>,
}

/// Something a user can open as a page.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PageTarget {
    /// An album.
    Album {
        /// The album.
        item: ItemId,
    },
    /// An artist.
    Artist {
        /// The artist.
        item: ItemId,
    },
    /// A user.
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
    /// A Provider View.
    View {
        /// The view.
        view: ViewRef,
    },
}

/// An artist's or a user's page. A part the Provider does not have is
/// `None` and is not shown.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfilePage {
    /// The artist or user.
    pub profile: ProfileView,
    /// Popular Tracks, most popular first.
    pub top_tracks: Option<Part>,
    /// Albums, EPs, singles and compilations.
    pub releases: Option<Part>,
    /// Every Track they published, newest first.
    pub tracks: Option<Part>,
    /// Their public playlists.
    pub playlists: Option<Part>,
    /// Releases by others that feature them.
    pub appears_on: Option<Part>,
    /// Tracks they liked, if public.
    pub likes: Option<Part>,
    /// What they reposted.
    pub reposts: Option<Part>,
    /// Related artists or users.
    pub related: Option<Part>,
    /// Everything they released, in order, for Play and Shuffle.
    pub discography: Option<CollectionRef>,
    /// Provider Views about them.
    pub extras: Vec<LinkView>,
}

/// How a section of a Provider View is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionLayout {
    /// A vertical list.
    Rows,
    /// A horizontal shelf.
    Shelf,
    /// A grid.
    Grid,
}

/// One block of a Provider View.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewSection {
    /// The heading.
    pub title: Option<String>,
    /// The layout.
    pub layout: SectionLayout,
    /// The content.
    pub content: Part,
}

/// A Provider View, laid out with the standard list UI.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewPage {
    /// The title.
    pub title: String,
    /// A second line.
    pub subtitle: Option<String>,
    /// Artwork renditions.
    pub artwork: Vec<ArtworkImage>,
    /// The sections, in order.
    pub sections: Vec<ViewSection>,
}

/// A page, as data. UIs own one layout per kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    /// An album and its Tracks.
    Album {
        /// The album.
        album: AlbumView,
        /// Its Tracks.
        tracks: Part,
    },
    /// A Provider Playlist and its Tracks.
    Playlist {
        /// The playlist.
        playlist: PlaylistView,
        /// Its Tracks.
        tracks: Part,
    },
    /// A Ristle Playlist and its Tracks.
    RistlePlaylist {
        /// The playlist.
        playlist: PlaylistSummary,
        /// Its Tracks.
        tracks: Part,
    },
    /// An artist.
    Artist {
        /// The page.
        page: ProfilePage,
    },
    /// A user.
    User {
        /// The page.
        page: ProfilePage,
    },
    /// A Provider View.
    View {
        /// The page.
        page: ViewPage,
    },
}

/// A standard part of a user's library on one Provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryKind {
    /// Liked or saved Tracks.
    LikedTracks,
    /// Saved albums.
    SavedAlbums,
    /// Followed artists.
    FollowedArtists,
    /// Followed users.
    FollowedUsers,
    /// Owned and followed playlists.
    Playlists,
    /// The user's own uploads.
    Uploads,
}

/// One shelf of the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    /// The kind of shelf.
    pub kind: LibraryKind,
    /// The title, as the Provider names it.
    pub title: String,
    /// The Provider (Plugin id) the shelf belongs to.
    pub provider: String,
    /// The collection to list and play.
    pub collection: CollectionRef,
}

/// The user's library across every Provider Connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryView {
    /// The shelves, grouped by kind and then by Provider.
    pub shelves: Vec<Shelf>,
    /// Provider Views shown in navigation, such as charts.
    pub views: Vec<LinkView>,
}
