//! Publisher keys and the package signature.

use std::fmt;

use sha2::{Digest, Sha256};

use super::PackageError;

/// A Publisher's Ed25519 public key, written `ed25519:<64 hex digits>` in
/// manifests, handovers and update feeds.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublisherKey([u8; 32]);

impl PublisherKey {
    /// The key's bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parses `ed25519:<hex>`.
    pub fn parse(text: &str) -> Result<Self, PackageError> {
        let invalid = || PackageError::Manifest(format!("invalid publisher key {text:?}"));
        let hex = text.strip_prefix("ed25519:").ok_or_else(invalid)?;
        let bytes = decode_hex(hex).ok_or_else(invalid)?;
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| invalid())?;
        // A key must be a valid curve point to verify anything.
        ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(|_| invalid())?;
        Ok(Self(bytes))
    }

    /// The key's bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// A short fingerprint for "Technical details": the first 16 hex digits
    /// of the key's SHA-256.
    pub fn fingerprint(&self) -> String {
        hex(&Sha256::digest(self.0)[..8])
    }

    /// Whether the package is signed by this key: one of the signatures in
    /// its `signature` section covers every byte after that section.
    pub(crate) fn signed(&self, package: &[u8]) -> bool {
        let mut raw = Vec::with_capacity(33);
        raw.push(0x01); // wasmsign2's Ed25519 key type
        raw.extend_from_slice(&self.0);
        let Ok(key) = wasmsign2::PublicKey::from_bytes(&raw) else {
            return false;
        };
        key.verify(&mut std::io::Cursor::new(package), None).is_ok()
    }

    /// Checks an Ed25519 signature by this key.
    pub(crate) fn verifies(&self, message: &[u8], signature: &[u8]) -> bool {
        let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&self.0) else {
            return false;
        };
        let Ok(signature) = ed25519_dalek::Signature::from_slice(signature) else {
            return false;
        };
        key.verify_strict(message, &signature).is_ok()
    }
}

impl fmt::Display for PublisherKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ed25519:{}", hex(&self.0))
    }
}

impl fmt::Debug for PublisherKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublisherKey({})", self.fingerprint())
    }
}

/// Lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Hex in either case, an even number of digits.
pub(crate) fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let digit = |byte: u8| char::from(byte).to_digit(16).map(|value| value as u8);
    text.as_bytes()
        .chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_bad_ones_are_refused() {
        let signing = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        let key = PublisherKey::from_bytes(signing.verifying_key().to_bytes());
        assert_eq!(PublisherKey::parse(&key.to_string()).unwrap(), key);
        assert_eq!(
            PublisherKey::parse(&key.to_string().to_uppercase().replace("ED25519", "ed25519"))
                .unwrap(),
            key
        );
        for bad in [
            "",
            "ed25519:",
            "ed25519:abc",
            "rsa:00",
            &format!("ed25519:{}", "zz".repeat(32)),
            &format!("ed25519:{}", "00".repeat(31)),
            &hex(&key.0),
        ] {
            assert!(PublisherKey::parse(bad).is_err(), "{bad:?}");
        }
        assert_eq!(key.fingerprint().len(), 16);
    }
}
