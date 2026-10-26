//! Deciding what installing a package means (`docs/plugin-api/packaging.md`,
//! "Installing" step 6, "Updates", "Key rotation", "Takeovers",
//! "Revocation").

use std::collections::BTreeSet;

use ristle_net::HostPattern;

use super::handover::Handover;
use super::key::PublisherKey;
use super::manifest::{Manifest, ShareKind};
use super::{Package, PackageError};

/// The optional interfaces a component exports. `catalog` is required, so
/// it is not listed. Read from the component's type at install time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Exports {
    /// `accounts`.
    pub accounts: bool,
    /// `likes`.
    pub likes: bool,
    /// `like-lookup`.
    pub like_lookup: bool,
    /// `playlist-editing`.
    pub playlist_editing: bool,
    /// `reposts`.
    pub reposts: bool,
    /// `feed`.
    pub feed: bool,
    /// `comments`.
    pub comments: bool,
    /// `settings`.
    pub settings: bool,
    /// `queue-projection`.
    pub queue_projection: bool,
    /// `byte-source`.
    pub byte_source: bool,
}

/// An export that adds a line to the consent screen. Read-only exports add
/// none, because everything they read is covered by the sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConsentExport {
    /// "Signs in as you and can do anything your account can on the
    /// service."
    Accounts,
    /// "Can like, save and follow for you."
    Likes,
    /// "Can repost for you."
    Reposts,
    /// "Can create, change and delete your playlists."
    PlaylistEditing,
    /// "Can show Ristle in the service's own apps and take commands from
    /// them."
    QueueProjection,
}

impl Exports {
    /// The exports that need consent.
    pub fn consent(&self) -> Vec<ConsentExport> {
        let mut out = Vec::new();
        if self.accounts {
            out.push(ConsentExport::Accounts);
        }
        if self.likes {
            out.push(ConsentExport::Likes);
        }
        if self.reposts {
            out.push(ConsentExport::Reposts);
        }
        if self.playlist_editing {
            out.push(ConsentExport::PlaylistEditing);
        }
        if self.queue_projection {
            out.push(ConsentExport::QueueProjection);
        }
        out
    }
}

/// What an update or downgrade asks for beyond what the user already
/// granted to the installed version. A new install has nothing to compare
/// against and shows the whole consent screen instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionDiff {
    /// New network destinations: `hosts`, `media-hosts` and `image-hosts`
    /// patterns not covered by any old one.
    pub hosts: Vec<String>,
    /// The update feed moved to, or appeared on, this host.
    pub update_host: Option<String>,
    /// New `sockets`.
    pub sockets: Vec<String>,
    /// New hosts the sign-in window may load.
    pub web_capture: Vec<String>,
    /// New hosts the sign-in window may watch and script, even when they
    /// were already loaded.
    pub web_capture_observe: Vec<String>,
    /// Each record kind that relays more fields to Party guests, with the
    /// fields it gains.
    pub party: Vec<(ShareKind, Vec<String>)>,
    /// Overlap turned on.
    pub overlap: bool,
    /// New `[auth] app-schemes`.
    pub app_schemes: Vec<String>,
    /// A larger `storage-mib`.
    pub storage_mib: Option<u32>,
    /// `servers` turned on.
    pub servers: bool,
    /// `folders` turned on.
    pub folders: bool,
    /// New exports that need consent.
    pub exports: Vec<ConsentExport>,
}

impl PermissionDiff {
    /// Whether nothing new is asked for, so an update installs without
    /// asking.
    pub fn is_empty(&self) -> bool {
        self == &PermissionDiff::default()
    }

    /// What `new` asks for that `old` did not.
    pub fn between(
        old: &Manifest,
        old_exports: &Exports,
        new: &Manifest,
        new_exports: &Exports,
    ) -> Self {
        let old_destinations: Vec<&HostPattern> = destinations(old).collect();
        let mut hosts = Vec::new();
        for pattern in destinations(new) {
            let text = pattern.to_string();
            if !old_destinations.iter().any(|old| old.covers(pattern)) && !hosts.contains(&text) {
                hosts.push(text);
            }
        }
        let update_host = match (&old.updates, &new.updates) {
            (_, None) => None,
            (Some(old), Some(new)) if old.host_str() == new.host_str() => None,
            (_, Some(new)) => new.host_str().map(str::to_owned),
        };
        let sockets = new
            .permissions
            .sockets
            .iter()
            .filter(|socket| !old.permissions.sockets.contains(socket))
            .map(ToString::to_string)
            .collect();
        let party = ShareKind::ALL
            .iter()
            .filter_map(|&kind| {
                let gained: Vec<String> = kind
                    .fields()
                    .iter()
                    .filter(|field| new.party.shares(kind, field) && !old.party.shares(kind, field))
                    .map(|field| (*field).to_owned())
                    .collect();
                (!gained.is_empty()).then_some((kind, gained))
            })
            .collect();
        let old_consent: BTreeSet<ConsentExport> = old_exports.consent().into_iter().collect();
        Self {
            hosts,
            update_host,
            sockets,
            web_capture: uncovered(&old.permissions.web_capture, &new.permissions.web_capture),
            web_capture_observe: uncovered(
                &old.permissions.web_capture_observe,
                &new.permissions.web_capture_observe,
            ),
            party,
            overlap: new.playback.overlap && !old.playback.overlap,
            app_schemes: new
                .auth
                .app_schemes
                .iter()
                .filter(|scheme| !old.auth.app_schemes.contains(scheme))
                .cloned()
                .collect(),
            storage_mib: (new.permissions.storage_mib > old.permissions.storage_mib)
                .then_some(new.permissions.storage_mib),
            servers: new.permissions.servers && !old.permissions.servers,
            folders: new.permissions.folders && !old.permissions.folders,
            exports: new_exports
                .consent()
                .into_iter()
                .filter(|export| !old_consent.contains(export))
                .collect(),
        }
    }
}

fn destinations(manifest: &Manifest) -> impl Iterator<Item = &HostPattern> {
    let permissions = &manifest.permissions;
    permissions
        .hosts
        .iter()
        .chain(&permissions.media_hosts)
        .chain(&permissions.image_hosts)
}

fn uncovered(old: &[HostPattern], new: &[HostPattern]) -> Vec<String> {
    new.iter()
        .filter(|pattern| !old.iter().any(|old| old.covers(pattern)))
        .map(ToString::to_string)
        .collect()
}

/// What the host knows about the installed version of a Plugin.
#[derive(Clone, Debug)]
pub struct Installed {
    /// Its manifest.
    pub manifest: Manifest,
    /// Its exports.
    pub exports: Exports,
    /// The Publisher key recorded for it: the key of the first install, or
    /// one the user accepted through a handover or a takeover. Not
    /// necessarily the key its manifest names.
    pub publisher_key: PublisherKey,
}

/// What installing a package would do, which decides the screen shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Review {
    /// Nothing with this id is installed: the whole consent screen.
    New,
    /// Signed by the installed Publisher key with a higher version. Installs
    /// without asking when `added` is empty and there is no handover.
    Update(Change),
    /// Signed by the installed Publisher key with a lower version: refused
    /// unless the user installs it by hand, with a warning.
    Downgrade(Change),
    /// Not signed by the installed Publisher key: someone other than the
    /// original maker offers a replacement.
    Takeover {
        /// A handover from the installed key naming the package's key, so
        /// the screen says the maker handed the Plugin over. Never from a
        /// revoked key.
        handover: Option<Handover>,
        /// The installed manifest's `publisher`, shown as text.
        from: String,
        /// The package's `publisher`, shown as text.
        to: String,
    },
}

/// An update or downgrade by the same Publisher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The installed version.
    pub from: semver::Version,
    /// What the package asks for that the installed version did not.
    pub added: PermissionDiff,
    /// A key rotation the user is asked to accept: the installed key
    /// signed a handover naming another key. Until it is accepted the
    /// installed key stays the Publisher key.
    pub handover: Option<Handover>,
    /// `servers = true` goes away: every Server Grant of the Plugin ends.
    pub ends_server_grants: bool,
    /// `folders = true` goes away: every Folder Grant of the Plugin ends.
    pub ends_folder_grants: bool,
}

/// Decides what installing `package` (with `exports`) over `installed`
/// means. `revoked` are the Publisher keys update feeds reported revoked;
/// nothing signed only by a revoked key is accepted, and handovers from one
/// are ignored.
pub fn review(
    package: &Package,
    exports: &Exports,
    installed: Option<&Installed>,
    revoked: &[PublisherKey],
) -> Result<Review, PackageError> {
    let manifest = package.manifest();
    let Some(installed) = installed else {
        if revoked.contains(&manifest.publisher_key) {
            return Err(PackageError::Revoked);
        }
        return Ok(Review::New);
    };
    if installed.manifest.id != manifest.id {
        return Err(PackageError::Manifest(
            "the package is another Plugin".to_owned(),
        ));
    }
    if package.signed_by(&installed.publisher_key) {
        // The installed key is among the signatures: this is the same
        // Publisher, whatever else signed it or the manifest says.
        if revoked.contains(&installed.publisher_key) {
            return Err(PackageError::Revoked);
        }
        let handover = package
            .handover()
            .filter(|handover| {
                handover.new_key != installed.publisher_key
                    && handover.signed_by(&installed.publisher_key, &manifest.id)
            })
            .cloned();
        let change = Change {
            from: installed.manifest.version.clone(),
            added: PermissionDiff::between(
                &installed.manifest,
                &installed.exports,
                manifest,
                exports,
            ),
            handover,
            ends_server_grants: installed.manifest.permissions.servers
                && !manifest.permissions.servers,
            ends_folder_grants: installed.manifest.permissions.folders
                && !manifest.permissions.folders,
        };
        // SemVer precedence: build metadata never orders releases.
        return match manifest.version.cmp_precedence(&installed.manifest.version) {
            std::cmp::Ordering::Greater => Ok(Review::Update(change)),
            std::cmp::Ordering::Less => Ok(Review::Downgrade(change)),
            std::cmp::Ordering::Equal => Err(PackageError::AlreadyInstalled),
        };
    }
    // Verified with the manifest's key when the package was read.
    if revoked.contains(&manifest.publisher_key) {
        return Err(PackageError::Revoked);
    }
    let handover = package
        .handover()
        .filter(|handover| {
            !revoked.contains(&installed.publisher_key)
                && handover.new_key == manifest.publisher_key
                && handover.signed_by(&installed.publisher_key, &manifest.id)
        })
        .cloned();
    Ok(Review::Takeover {
        handover,
        from: installed.manifest.publisher.clone(),
        to: manifest.publisher.clone(),
    })
}
