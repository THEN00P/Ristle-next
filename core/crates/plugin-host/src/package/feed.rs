//! Update feeds (`docs/plugin-api/packaging.md`, "Update feeds").
//!
//! A feed adds no trust of its own: a release it offers is downloaded,
//! checked against its `sha256`, and then reviewed like any package the user
//! picked. This module parses a feed and picks what to offer; fetching it
//! (HTTPS on the `updates` host only, at most [`MAX_FEED_BYTES`] within
//! [`FEED_TIMEOUT`]) uses `ristle-net` with a reach of that one host.

use std::time::Duration;

use ristle_domain::PluginId;
use serde::Deserialize;
use url::Url;

use super::PackageError;
use super::handover::Revocation;
use super::key::{PublisherKey, decode_hex};

/// The largest feed read: 256 KiB.
pub const MAX_FEED_BYTES: u64 = 256 * 1024;

/// How long a feed may take to arrive.
pub const FEED_TIMEOUT: Duration = Duration::from_secs(15);

/// A parsed feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateFeed {
    /// Releases whose download URL is on the feed's host, newest first.
    pub releases: Vec<Release>,
    /// Keys the feed's revocations genuinely revoke.
    pub revoked: Vec<PublisherKey>,
}

/// One release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Its version.
    pub version: semver::Version,
    /// Where to download it.
    pub url: Url,
    /// The SHA-256 the download must have.
    pub sha256: [u8; 32],
    /// The `ristle:plugin` version it needs, such as `0.1`.
    pub api: String,
    /// Release notes, as text.
    pub notes: Option<String>,
}

/// What a feed offers an installed Plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Offer {
    /// Nothing newer.
    Current,
    /// A newer release this Ristle can run.
    Update(Release),
    /// Only releases that need a newer Ristle; the newest such version.
    NeedsNewerRistle(semver::Version),
}

#[derive(Deserialize)]
struct RawFeed {
    id: String,
    // Entries are read one by one, so a malformed one costs only itself.
    #[serde(default)]
    releases: Vec<serde_json::Value>,
    #[serde(default)]
    revoked: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawRelease {
    version: String,
    url: String,
    sha256: String,
    api: String,
    notes: Option<String>,
}

/// Parses a feed fetched from `updates` for the Plugin `id`. Releases whose
/// URL is not HTTPS on the feed's host, or that are malformed, are ignored;
/// revocations that do not verify are ignored.
pub fn parse(body: &[u8], id: &PluginId, updates: &Url) -> Result<UpdateFeed, PackageError> {
    if body.len() as u64 > MAX_FEED_BYTES {
        return Err(PackageError::Feed("larger than 256 KiB".to_owned()));
    }
    let raw: RawFeed = serde_json::from_slice(body)
        .map_err(|error| PackageError::Feed(format!("not a feed: {error}")))?;
    if raw.id != id.as_str() {
        return Err(PackageError::Feed(
            "the feed is for another Plugin".to_owned(),
        ));
    }
    let mut releases: Vec<Release> = raw
        .releases
        .into_iter()
        .filter_map(|release| {
            let release: RawRelease = serde_json::from_value(release).ok()?;
            let url = Url::parse(&release.url).ok()?;
            let on_feed_host = url.scheme() == "https"
                && url.host_str() == updates.host_str()
                && url.port_or_known_default() == updates.port_or_known_default()
                && url.username().is_empty()
                && url.password().is_none();
            if !on_feed_host {
                tracing::warn!(plugin = %id, "ignoring a release outside the feed's host");
                return None;
            }
            let sha256: [u8; 32] = decode_hex(&release.sha256)?.try_into().ok()?;
            Some(Release {
                version: semver::Version::parse(&release.version).ok()?,
                url,
                sha256,
                api: release.api,
                notes: release.notes,
            })
        })
        .collect();
    releases.sort_by(|a, b| b.version.cmp_precedence(&a.version));
    let revoked = raw
        .revoked
        .into_iter()
        .filter_map(|revocation| serde_json::from_value::<Revocation>(revocation).ok())
        .filter_map(|revocation| revocation.verified())
        .collect();
    Ok(UpdateFeed { releases, revoked })
}

impl UpdateFeed {
    /// The newest release above `installed` that needs one of the `apis`
    /// this host implements.
    pub fn offer(&self, installed: &semver::Version, apis: &[&str]) -> Offer {
        let newer = self
            .releases
            .iter()
            .filter(|release| release.version.cmp_precedence(installed).is_gt());
        let mut needs_newer = None;
        for release in newer {
            if apis.contains(&release.api.as_str()) {
                return Offer::Update(release.clone());
            }
            needs_newer.get_or_insert_with(|| release.version.clone());
        }
        match needs_newer {
            Some(version) => Offer::NeedsNewerRistle(version),
            None => Offer::Current,
        }
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::package::key::hex;

    fn feed(json: &str) -> Result<UpdateFeed, PackageError> {
        parse(
            json.as_bytes(),
            &PluginId::new("dev.example.soundcloud"),
            &Url::parse("https://example.com/ristle/soundcloud.json").unwrap(),
        )
    }

    const SHA: &str = "9f2c000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn releases_off_the_feed_host_are_ignored() {
        let json = format!(
            r#"{{"id":"dev.example.soundcloud","releases":[
                {{"version":"1.5.0","url":"https://example.com/a.wasm","sha256":"{SHA}","api":"0.1","notes":"x"}},
                {{"version":"1.6.0","url":"https://evil.example/a.wasm","sha256":"{SHA}","api":"0.1"}},
                {{"version":"1.7.0","url":"http://example.com/a.wasm","sha256":"{SHA}","api":"0.1"}},
                {{"version":"1.8.0","url":"https://example.com/a.wasm","sha256":"00","api":"0.1"}},
                {{"version":"one","url":"https://example.com/a.wasm","sha256":"{SHA}","api":"0.1"}}
            ]}}"#
        );
        let feed = feed(&json).unwrap();
        assert_eq!(feed.releases.len(), 1);
        assert_eq!(feed.releases[0].version, semver::Version::new(1, 5, 0));
    }

    #[test]
    fn the_newest_runnable_release_is_offered() {
        let json = format!(
            r#"{{"id":"dev.example.soundcloud","releases":[
                {{"version":"1.5.0","url":"https://example.com/a.wasm","sha256":"{SHA}","api":"0.1"}},
                {{"version":"2.0.0","url":"https://example.com/b.wasm","sha256":"{SHA}","api":"0.2"}},
                {{"version":"1.6.0","url":"https://example.com/c.wasm","sha256":"{SHA}","api":"0.1"}}
            ]}}"#
        );
        let feed = feed(&json).unwrap();
        let v = |text: &str| semver::Version::parse(text).unwrap();
        match feed.offer(&v("1.4.0"), &["0.1"]) {
            Offer::Update(release) => assert_eq!(release.version, v("1.6.0")),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            feed.offer(&v("1.6.0"), &["0.1"]),
            Offer::NeedsNewerRistle(v("2.0.0"))
        );
        assert_eq!(feed.offer(&v("2.0.0"), &["0.1"]), Offer::Current);
    }

    #[test]
    fn a_malformed_entry_costs_only_itself() {
        let signing = SigningKey::from_bytes(&[9; 32]);
        let key = PublisherKey::from_bytes(signing.verifying_key().to_bytes());
        let at = "2026-11-02T10:00:00Z";
        let good = hex(&signing.sign(&Revocation::message(&key, at)).to_bytes());
        let json = format!(
            r#"{{"id":"dev.example.soundcloud","releases":[
                {{"version":"1.5.0","url":"https://example.com/a.wasm"}},
                {{"version":15,"url":"https://example.com/a.wasm","sha256":"{SHA}","api":"0.1"}},
                "nonsense",
                {{"version":"1.6.0","url":"https://example.com/b.wasm","sha256":"{SHA}","api":"0.1"}}
            ],"revoked":[
                {{"key":"{key}"}},
                {{"key":"{key}","at":"{at}","signature":"{good}"}}
            ]}}"#
        );
        let feed = feed(&json).unwrap();
        assert_eq!(feed.releases.len(), 1);
        assert_eq!(feed.releases[0].version, semver::Version::new(1, 6, 0));
        assert_eq!(feed.revoked, vec![key]);
    }

    #[test]
    fn build_metadata_never_makes_a_release_newer() {
        let json = format!(
            r#"{{"id":"dev.example.soundcloud","releases":[
                {{"version":"1.5.0+build.2","url":"https://example.com/a.wasm","sha256":"{SHA}","api":"0.1"}}
            ]}}"#
        );
        let feed = feed(&json).unwrap();
        let installed = semver::Version::parse("1.5.0+build.1").unwrap();
        assert_eq!(feed.offer(&installed, &["0.1"]), Offer::Current);
    }

    #[test]
    fn feeds_for_another_plugin_or_too_large_are_refused() {
        assert!(feed(r#"{"id":"dev.example.other","releases":[]}"#).is_err());
        assert!(feed("not json").is_err());
        let big = format!(
            r#"{{"id":"dev.example.soundcloud","pad":"{}"}}"#,
            "x".repeat(MAX_FEED_BYTES as usize)
        );
        assert!(feed(&big).is_err());
    }

    #[test]
    fn only_genuine_revocations_count() {
        let signing = SigningKey::from_bytes(&[9; 32]);
        let key = PublisherKey::from_bytes(signing.verifying_key().to_bytes());
        let at = "2026-11-02T10:00:00Z";
        let good = hex(&signing.sign(&Revocation::message(&key, at)).to_bytes());
        let json = format!(
            r#"{{"id":"dev.example.soundcloud","revoked":[
                {{"key":"{key}","at":"{at}","signature":"{good}"}},
                {{"key":"{key}","at":"{at}","signature":"{}"}}
            ]}}"#,
            "00".repeat(64)
        );
        assert_eq!(feed(&json).unwrap().revoked, vec![key]);
    }
}
