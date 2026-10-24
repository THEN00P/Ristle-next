//! Plugin Packages (`docs/plugin-api/packaging.md`).
//!
//! [`Package::read`] runs install steps 1 to 4: read the sections, take the
//! untrusted manifest's `id` and `publisher-key`, verify the `wasmsign2`
//! signature with that key, then parse and validate the whole manifest and
//! the icon. [`review`] is step 6: new install, update, downgrade or
//! takeover, with the permission difference. Step 5, checking the
//! component's imports and exports, needs the engine.

mod feed;
mod handover;
mod icon;
mod key;
mod manifest;
mod review;
mod sections;

use std::sync::Arc;

use sha2::{Digest, Sha256};

pub use feed::{FEED_TIMEOUT, MAX_FEED_BYTES, Offer, Release, UpdateFeed, parse as parse_feed};
pub use handover::{Handover, Revocation};
#[cfg(feature = "test-support")]
pub use icon::png_header;
pub use icon::{MAX_ICON_SIDE, MIN_ICON_SIDE};
pub use key::PublisherKey;
pub use manifest::{
    Auth, DEFAULT_STORAGE_MIB, Limits, MAX_STORAGE_MIB, Manifest, Party, Permissions, Playback,
    ProviderInfo, Redirect, ShareKind, SocketTarget,
};
pub use review::{Change, ConsentExport, Exports, Installed, PermissionDiff, Review, review};
pub use sections::MAX_PACKAGE_BYTES;

/// Why a package was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PackageError {
    /// The file is not a WebAssembly component.
    #[error("not a WebAssembly component")]
    NotAComponent,
    /// The file breaks off or its sections are malformed.
    #[error("malformed package")]
    Malformed,
    /// The file or one of its sections is over its limit.
    #[error("{0} too large")]
    TooLarge(&'static str),
    /// The first section is not a signature.
    #[error("the package is not signed")]
    Unsigned,
    /// The Ristle sections are missing, repeated or out of order.
    #[error("bad package layout: {0}")]
    Layout(&'static str),
    /// No signature by the manifest's `publisher-key` covers the package.
    #[error("the signature does not match the publisher key")]
    BadSignature,
    /// The manifest is invalid.
    #[error("invalid manifest: {0}")]
    Manifest(String),
    /// The icon is not a square PNG of 256 to 1024 pixels.
    #[error("invalid icon: {0}")]
    Icon(String),
    /// The handover section is malformed.
    #[error("invalid handover: {0}")]
    Handover(String),
    /// The update feed is unusable.
    #[error("invalid update feed: {0}")]
    Feed(String),
    /// The key it is signed with was revoked by its Publisher.
    #[error("the publisher key was revoked")]
    Revoked,
    /// This version is installed already.
    #[error("this version is already installed")]
    AlreadyInstalled,
    /// The component cannot run on this host: it does not compile, imports
    /// something Ristle does not provide, lacks `catalog`, or cannot be
    /// instantiated within the instance limits.
    #[error("the component cannot run here: {0}")]
    Component(String),
    /// The component is built for a `ristle:plugin` version this Ristle
    /// does not implement.
    #[error("this plugin needs another version of Ristle: {0}")]
    NeedsNewerRistle(String),
}

/// Lowercase hex of a package hash, as in artifact file names.
pub(crate) fn hex_digest(digest: &[u8; 32]) -> String {
    key::hex(digest)
}

/// A package whose signature and manifest have been checked.
#[derive(Clone, Debug)]
pub struct Package {
    bytes: Arc<[u8]>,
    sha256: [u8; 32],
    manifest: Manifest,
    icon: Option<Vec<u8>>,
    handover: Option<Handover>,
}

/// Two packages are the same when their bytes are.
impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.sha256 == other.sha256 && self.bytes == other.bytes
    }
}

impl Eq for Package {}

impl Package {
    /// Reads and verifies a package (install steps 1 to 4).
    pub fn read(bytes: Vec<u8>) -> Result<Self, PackageError> {
        let parsed = sections::read(&bytes)?;
        let (_, key) = manifest::identity(parsed.manifest)?;
        if !key.signed(&bytes) {
            return Err(PackageError::BadSignature);
        }
        let manifest = manifest::parse(parsed.manifest)?;
        let icon = match parsed.icon {
            Some(icon) => {
                icon::check(icon)?;
                Some(icon.to_vec())
            }
            None => None,
        };
        let handover = parsed.handover.map(Handover::parse).transpose()?;
        let sha256 = Sha256::digest(&bytes).into();
        Ok(Self {
            bytes: Arc::from(bytes),
            sha256,
            manifest,
            icon,
            handover,
        })
    }

    /// The whole file, as stored and compiled.
    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }

    /// SHA-256 of the whole file: the package hash that keys compiled
    /// artifacts and is checked against an update feed.
    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    /// The validated manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The icon's PNG bytes, checked to be square and 256 to 1024 pixels.
    pub fn icon(&self) -> Option<&[u8]> {
        self.icon.as_deref()
    }

    /// The handover section, parsed but not yet checked against a key.
    pub fn handover(&self) -> Option<&Handover> {
        self.handover.as_ref()
    }

    /// Whether `key` is among the package's signers.
    pub fn signed_by(&self, key: &PublisherKey) -> bool {
        key.signed(&self.bytes)
    }
}
