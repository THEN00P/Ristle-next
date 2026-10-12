//! Plugin Package format: one `.wasm` component carrying its manifest and an
//! Ed25519 signature in custom sections.
//!
//! The signature section must be the last top-level section. It signs the
//! SHA-256 of every byte before it, so the manifest, the code and every other
//! custom section are covered, and nothing may follow it.

use std::collections::HashMap;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MANIFEST_SECTION: &str = "ristle-manifest";
pub const SIGNATURE_SECTION: &str = "ristle-signature";
const SIGNATURE_VERSION: u8 = 1;
const DOMAIN: &[u8] = b"ristle-plugin-signature-v1\0";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub hosts: Vec<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum VerifyError {
    Malformed(String),
    MissingManifest,
    DuplicateManifest,
    Unsigned,
    SignatureNotLast,
    BadSignature,
}

pub struct Verified {
    pub manifest: Manifest,
    pub publisher: [u8; 32],
}

/// Adds the manifest as a custom section. The package must not be signed yet.
pub fn embed_manifest(component: &[u8], manifest: &Manifest) -> Vec<u8> {
    let mut out = component.to_vec();
    push_custom_section(&mut out, MANIFEST_SECTION, &serde_json::to_vec(manifest).expect("manifest json"));
    out
}

/// Appends the signature section over everything before it.
pub fn sign(package: &[u8], key: &SigningKey) -> Vec<u8> {
    let signature = key.sign(&message(package));
    let mut payload = vec![SIGNATURE_VERSION];
    payload.extend_from_slice(key.verifying_key().as_bytes());
    payload.extend_from_slice(&signature.to_bytes());
    let mut out = package.to_vec();
    push_custom_section(&mut out, SIGNATURE_SECTION, &payload);
    out
}

/// Checks the structure and the signature and returns the signed manifest.
/// Publisher continuity is the caller's decision, see [`TrustStore`].
pub fn verify(bytes: &[u8]) -> Result<Verified, VerifyError> {
    let sections = sections(bytes)?;
    let (last, rest) = sections.split_last().ok_or(VerifyError::Unsigned)?;
    if rest.iter().any(|s| s.name.as_deref() == Some(SIGNATURE_SECTION)) {
        return Err(VerifyError::SignatureNotLast);
    }
    if last.name.as_deref() != Some(SIGNATURE_SECTION) {
        return Err(VerifyError::Unsigned);
    }
    let payload = &bytes[last.payload.clone()];
    if payload.len() != 1 + 32 + 64 || payload[0] != SIGNATURE_VERSION {
        return Err(VerifyError::Malformed("signature section".into()));
    }
    let publisher: [u8; 32] = payload[1..33].try_into().unwrap();
    let key = VerifyingKey::from_bytes(&publisher).map_err(|_| VerifyError::BadSignature)?;
    let signature = Signature::from_bytes(payload[33..97].try_into().unwrap());
    key.verify(&message(&bytes[..last.start]), &signature).map_err(|_| VerifyError::BadSignature)?;

    let mut manifests = rest.iter().filter(|s| s.name.as_deref() == Some(MANIFEST_SECTION));
    let manifest = manifests.next().ok_or(VerifyError::MissingManifest)?;
    if manifests.next().is_some() {
        return Err(VerifyError::DuplicateManifest);
    }
    let manifest = serde_json::from_slice(&bytes[manifest.payload.clone()])
        .map_err(|e| VerifyError::Malformed(format!("manifest: {e}")))?;
    Ok(Verified { manifest, publisher })
}

/// What installing a verified package means for an installed Plugin.
#[derive(Debug, PartialEq)]
pub enum Admission {
    NewInstall,
    /// Same Plugin id, same Publisher key.
    Update,
    /// Same Plugin id, different key. Needs the user's explicit acceptance.
    Takeover { installed: [u8; 32] },
}

/// Installed Publisher keys by Plugin id.
#[derive(Default)]
pub struct TrustStore {
    publishers: HashMap<String, [u8; 32]>,
}

impl TrustStore {
    pub fn admission(&self, package: &Verified) -> Admission {
        match self.publishers.get(&package.manifest.id) {
            None => Admission::NewInstall,
            Some(key) if *key == package.publisher => Admission::Update,
            Some(key) => Admission::Takeover { installed: *key },
        }
    }

    pub fn install(&mut self, package: &Verified) {
        self.publishers.insert(package.manifest.id.clone(), package.publisher);
    }
}

fn message(signed: &[u8]) -> Vec<u8> {
    let mut m = DOMAIN.to_vec();
    m.extend_from_slice(&Sha256::digest(signed));
    m
}

struct Section {
    start: usize,
    name: Option<String>,
    payload: std::ops::Range<usize>,
}

/// Top-level sections of a core module or a component. Nested modules are
/// inside their section's payload and are not walked.
fn sections(bytes: &[u8]) -> Result<Vec<Section>, VerifyError> {
    if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
        return Err(VerifyError::Malformed("not a wasm binary".into()));
    }
    let mut pos = 8;
    let mut out = Vec::new();
    while pos < bytes.len() {
        let start = pos;
        let id = bytes[pos];
        pos += 1;
        let size = read_leb(bytes, &mut pos)? as usize;
        let end = pos
            .checked_add(size)
            .filter(|&e| e <= bytes.len())
            .ok_or_else(|| VerifyError::Malformed("section size".into()))?;
        let (name, payload) = if id == 0 {
            let mut p = pos;
            let len = read_leb(bytes, &mut p)? as usize;
            let name_end = p
                .checked_add(len)
                .filter(|&e| e <= end)
                .ok_or_else(|| VerifyError::Malformed("section name".into()))?;
            let name = std::str::from_utf8(&bytes[p..name_end]).map_err(|_| VerifyError::Malformed("section name".into()))?;
            (Some(name.to_string()), name_end..end)
        } else {
            (None, pos..end)
        };
        out.push(Section { start, name, payload });
        pos = end;
    }
    Ok(out)
}

fn read_leb(bytes: &[u8], pos: &mut usize) -> Result<u32, VerifyError> {
    let mut result = 0u32;
    for shift in (0..35).step_by(7) {
        let b = *bytes.get(*pos).ok_or_else(|| VerifyError::Malformed("truncated".into()))?;
        *pos += 1;
        result |= u32::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(VerifyError::Malformed("leb128".into()))
}

fn push_leb(out: &mut Vec<u8>, mut v: u32) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

pub fn push_custom_section(out: &mut Vec<u8>, name: &str, payload: &[u8]) {
    let mut body = Vec::new();
    push_leb(&mut body, name.len() as u32);
    body.extend_from_slice(name.as_bytes());
    body.extend_from_slice(payload);
    out.push(0);
    push_leb(out, body.len() as u32);
    out.extend_from_slice(&body);
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_COMPONENT: &[u8] = b"\0asm\x0d\0\x01\0";

    fn manifest(id: &str) -> Manifest {
        Manifest {
            id: id.into(),
            name: "Test".into(),
            version: "1.0.0".into(),
            author: "Ristle".into(),
            hosts: vec!["api.example.com".into()],
            capabilities: vec![],
        }
    }

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn round_trip() {
        let signed = sign(&embed_manifest(EMPTY_COMPONENT, &manifest("a")), &key(1));
        let v = verify(&signed).unwrap();
        assert_eq!(v.manifest, manifest("a"));
        assert_eq!(v.publisher, *key(1).verifying_key().as_bytes());
    }

    #[test]
    fn rejects_any_flipped_byte_before_the_signature() {
        let unsigned = embed_manifest(EMPTY_COMPONENT, &manifest("a"));
        let signed = sign(&unsigned, &key(1));
        for i in 8..unsigned.len() {
            let mut t = signed.clone();
            t[i] ^= 1;
            assert!(verify(&t).is_err(), "byte {i}");
        }
    }

    #[test]
    fn rejects_unsigned_and_trailing_sections() {
        let unsigned = embed_manifest(EMPTY_COMPONENT, &manifest("a"));
        assert_eq!(verify(&unsigned).err(), Some(VerifyError::Unsigned));
        let mut trailing = sign(&unsigned, &key(1));
        push_custom_section(&mut trailing, "extra", b"x");
        assert_eq!(verify(&trailing).err(), Some(VerifyError::SignatureNotLast));
        // Wrapping a signed package in a second signature is not allowed either.
        let resigned = sign(&trailing, &key(1));
        assert_eq!(verify(&resigned).err(), Some(VerifyError::SignatureNotLast));
    }

    #[test]
    fn takeover_is_detected() {
        let mut trust = TrustStore::default();
        let original = verify(&sign(&embed_manifest(EMPTY_COMPONENT, &manifest("a")), &key(1))).unwrap();
        assert_eq!(trust.admission(&original), Admission::NewInstall);
        trust.install(&original);
        let update = verify(&sign(&embed_manifest(EMPTY_COMPONENT, &manifest("a")), &key(1))).unwrap();
        assert_eq!(trust.admission(&update), Admission::Update);
        let other = verify(&sign(&embed_manifest(EMPTY_COMPONENT, &manifest("a")), &key(2))).unwrap();
        assert!(matches!(trust.admission(&other), Admission::Takeover { .. }));
    }
}
