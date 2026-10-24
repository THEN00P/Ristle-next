//! Handovers and revocations (`docs/plugin-api/packaging.md`, "Key
//! rotation", "Takeovers" and "Revocation").
//!
//! Both are signed with plain Ed25519 over a domain-separated message, so a
//! signature made for one can never pass as the other or as a package
//! signature.

use ristle_domain::PluginId;
use serde::Deserialize;

use super::PackageError;
use super::key::{PublisherKey, decode_hex, hex};

const HANDOVER_DOMAIN: &[u8] = b"ristle-handover-v1\0";
const REVOCATION_DOMAIN: &[u8] = b"ristle-revocation-v1\0";

/// A `ristle-handover` section: the previous Publisher key's signed
/// statement that `new_key` now speaks for the Plugin.
///
/// ```toml
/// id = "dev.example.soundcloud"
/// new-key = "ed25519:…"
/// issued-at = 1760000000000   # Unix milliseconds
/// signature = "…"             # hex, by the previous key
/// ```
///
/// The signature covers `"ristle-handover-v1\0" ‖ id ‖ 0x00 ‖ new key (32
/// bytes) ‖ issued-at (u64, big-endian)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handover {
    /// The Plugin it is for.
    pub id: PluginId,
    /// The key it hands the Plugin to.
    pub new_key: PublisherKey,
    /// When the previous key signed it, as it says.
    pub issued_at_ms: u64,
    signature: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawHandover {
    id: String,
    new_key: String,
    issued_at: u64,
    signature: String,
}

impl Handover {
    /// Parses a handover section. Its signature is checked by
    /// [`Handover::signed_by`].
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, PackageError> {
        let invalid = |why: &str| PackageError::Handover(why.to_owned());
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("not UTF-8"))?;
        let raw: RawHandover = toml::from_str(text).map_err(|error| invalid(error.message()))?;
        let signature = decode_hex(&raw.signature)
            .filter(|signature| signature.len() == 64)
            .ok_or_else(|| invalid("signature is not 64 bytes of hex"))?;
        Ok(Self {
            id: PluginId::new(raw.id),
            new_key: PublisherKey::parse(&raw.new_key).map_err(|_| invalid("invalid new-key"))?,
            issued_at_ms: raw.issued_at,
            signature,
        })
    }

    /// The signed message.
    pub(crate) fn message(id: &PluginId, new_key: &PublisherKey, issued_at_ms: u64) -> Vec<u8> {
        let mut message = Vec::with_capacity(HANDOVER_DOMAIN.len() + id.as_str().len() + 41);
        message.extend_from_slice(HANDOVER_DOMAIN);
        message.extend_from_slice(id.as_str().as_bytes());
        message.push(0);
        message.extend_from_slice(new_key.as_bytes());
        message.extend_from_slice(&issued_at_ms.to_be_bytes());
        message
    }

    /// Whether `previous` signed it for `id`.
    pub(crate) fn signed_by(&self, previous: &PublisherKey, id: &PluginId) -> bool {
        &self.id == id
            && previous.verifies(
                &Self::message(&self.id, &self.new_key, self.issued_at_ms),
                &self.signature,
            )
    }

    /// The section text for a handover with this signature, for the packing
    /// tool and tests.
    pub fn to_toml(&self) -> String {
        format!(
            "id = \"{}\"\nnew-key = \"{}\"\nissued-at = {}\nsignature = \"{}\"\n",
            self.id,
            self.new_key,
            self.issued_at_ms,
            hex(&self.signature)
        )
    }

    /// A handover with a signature made elsewhere.
    pub fn with_signature(
        id: PluginId,
        new_key: PublisherKey,
        issued_at_ms: u64,
        signature: [u8; 64],
    ) -> Self {
        Self {
            id,
            new_key,
            issued_at_ms,
            signature: signature.to_vec(),
        }
    }

    /// The bytes the previous key signs.
    pub fn signing_message(&self) -> Vec<u8> {
        Self::message(&self.id, &self.new_key, self.issued_at_ms)
    }
}

/// A revocation from an update feed, signed by the revoked key itself, so
/// anyone holding the key can revoke it, which only ever removes trust.
///
/// The signature covers `"ristle-revocation-v1\0" ‖ key (32 bytes) ‖ at`,
/// with `at` the feed's text as it stands.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Revocation {
    /// `ed25519:<hex>`.
    pub key: String,
    /// When the leak was reported, as the feed writes it.
    pub at: String,
    /// Hex.
    pub signature: String,
}

impl Revocation {
    /// The revoked key, when the revocation is genuine.
    pub fn verified(&self) -> Option<PublisherKey> {
        let key = PublisherKey::parse(&self.key).ok()?;
        let signature = decode_hex(&self.signature)?;
        key.verifies(&Self::message(&key, &self.at), &signature)
            .then_some(key)
    }

    /// The signed message.
    pub fn message(key: &PublisherKey, at: &str) -> Vec<u8> {
        let mut message = Vec::with_capacity(REVOCATION_DOMAIN.len() + 32 + at.len());
        message.extend_from_slice(REVOCATION_DOMAIN);
        message.extend_from_slice(key.as_bytes());
        message.extend_from_slice(at.as_bytes());
        message
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn key(seed: u8) -> (SigningKey, PublisherKey) {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        let public = PublisherKey::from_bytes(signing.verifying_key().to_bytes());
        (signing, public)
    }

    #[test]
    fn a_handover_verifies_only_for_its_signer_and_id() {
        let (old_signing, old) = key(1);
        let (_, new) = key(2);
        let id = PluginId::new("dev.example.x");
        let message = Handover::message(&id, &new, 1_000);
        let handover = Handover::with_signature(
            id.clone(),
            new,
            1_000,
            old_signing.sign(&message).to_bytes(),
        );
        let parsed = Handover::parse(handover.to_toml().as_bytes()).unwrap();
        assert_eq!(parsed, handover);
        assert!(parsed.signed_by(&old, &id));
        assert!(!parsed.signed_by(&new, &id));
        assert!(!parsed.signed_by(&old, &PluginId::new("dev.example.y")));
        // A changed date breaks the signature.
        let backdated = Handover {
            issued_at_ms: 999,
            ..parsed
        };
        assert!(!backdated.signed_by(&old, &id));
    }

    #[test]
    fn malformed_handovers_are_refused() {
        for text in [
            "id = \"x\"",
            "id = \"x\"\nnew-key = \"ed25519:00\"\nissued-at = 1\nsignature = \"00\"",
            "\u{0}",
        ] {
            assert!(matches!(
                Handover::parse(text.as_bytes()),
                Err(PackageError::Handover(_))
            ));
        }
    }

    #[test]
    fn revocations_are_signed_by_the_revoked_key() {
        let (signing, public) = key(3);
        let at = "2026-11-02T10:00:00Z";
        let signature = signing.sign(&Revocation::message(&public, at)).to_bytes();
        let revocation = Revocation {
            key: public.to_string(),
            at: at.to_owned(),
            signature: hex(&signature),
        };
        assert_eq!(revocation.verified(), Some(public));
        let (other, _) = key(4);
        let forged = Revocation {
            signature: hex(&other.sign(&Revocation::message(&public, at)).to_bytes()),
            ..revocation.clone()
        };
        assert_eq!(forged.verified(), None);
        let moved = Revocation {
            at: "2026-11-03T10:00:00Z".to_owned(),
            ..revocation
        };
        assert_eq!(moved.verified(), None);
    }
}
