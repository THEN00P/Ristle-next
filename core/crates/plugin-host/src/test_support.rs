//! Signed packages for tests: the steps `ristle-plugin pack` performs,
//! with keys derived from a seed.

use ed25519_dalek::{Signer, SigningKey};
use ristle_domain::PluginId;

use crate::package::{Handover, PublisherKey, Revocation};

/// The preamble of an empty WebAssembly component.
pub const EMPTY_COMPONENT: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];

/// A Publisher's signing key.
#[derive(Clone)]
pub struct Publisher {
    signing: SigningKey,
}

impl std::fmt::Debug for Publisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Publisher").field(&self.key()).finish()
    }
}

impl Publisher {
    /// A key derived from `seed`.
    pub fn new(seed: u8) -> Self {
        Self {
            signing: SigningKey::from_bytes(&[seed; 32]),
        }
    }

    /// The public key.
    pub fn key(&self) -> PublisherKey {
        PublisherKey::from_bytes(self.signing.verifying_key().to_bytes())
    }

    /// A handover section by this key, naming `to`.
    pub fn handover(&self, id: &str, to: &PublisherKey, issued_at_ms: u64) -> String {
        let id = PluginId::new(id);
        let message =
            Handover::with_signature(id.clone(), *to, issued_at_ms, [0; 64]).signing_message();
        Handover::with_signature(
            id,
            *to,
            issued_at_ms,
            self.signing.sign(&message).to_bytes(),
        )
        .to_toml()
    }

    /// A revocation of this key.
    pub fn revocation(&self, at: &str) -> Revocation {
        let signature = self
            .signing
            .sign(&Revocation::message(&self.key(), at))
            .to_bytes();
        Revocation {
            key: self.key().to_string(),
            at: at.to_owned(),
            signature: signature.iter().map(|byte| format!("{byte:02x}")).collect(),
        }
    }

    fn wasmsign2_key(&self) -> wasmsign2::SecretKey {
        let mut raw = vec![0x81]; // wasmsign2's Ed25519 secret key type
        raw.extend_from_slice(&self.signing.to_keypair_bytes());
        match wasmsign2::SecretKey::from_bytes(&raw) {
            Ok(key) => key,
            // An Ed25519 keypair always converts.
            Err(error) => unreachable_key(error),
        }
    }
}

#[allow(clippy::panic)]
fn unreachable_key(error: wasmsign2::WSError) -> ! {
    panic!("wasmsign2 refused an Ed25519 keypair: {error}")
}

/// A manifest for tests: `extra` is appended to the top-level keys, before
/// the `[provider]` table.
pub fn manifest(
    id: &str,
    version: &str,
    publisher: &str,
    key: &PublisherKey,
    extra: &str,
) -> String {
    format!(
        "id = \"{id}\"\nname = \"Example\"\nversion = \"{version}\"\npublisher = \"{publisher}\"\npublisher-key = \"{key}\"\n{extra}\n\n[provider]\nid = \"example\"\nname = \"Example\"\n"
    )
}

/// The pieces of a package before signing.
#[derive(Clone, Debug)]
pub struct Unsigned {
    /// The component. Its preamble is replaced by the component preamble.
    pub component: Vec<u8>,
    /// The manifest TOML.
    pub manifest: String,
    /// The icon section.
    pub icon: Option<Vec<u8>>,
    /// The handover section.
    pub handover: Option<String>,
}

impl Unsigned {
    /// An empty component with this manifest.
    pub fn new(manifest: String) -> Self {
        Self {
            component: EMPTY_COMPONENT.to_vec(),
            manifest,
            icon: None,
            handover: None,
        }
    }

    /// The bytes with the Ristle sections in front of the component's own,
    /// unsigned.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = EMPTY_COMPONENT.to_vec();
        custom_section(&mut out, "ristle-manifest", self.manifest.as_bytes());
        if let Some(icon) = &self.icon {
            custom_section(&mut out, "ristle-icon", icon);
        }
        if let Some(handover) = &self.handover {
            custom_section(&mut out, "ristle-handover", handover.as_bytes());
        }
        out.extend_from_slice(self.component.get(8..).unwrap_or_default());
        out
    }

    /// Signed by each Publisher in turn, all signatures in one section.
    pub fn signed(&self, by: &[&Publisher]) -> Vec<u8> {
        sign(self.bytes(), by)
    }
}

/// Adds signatures to a package, signed or not.
#[allow(clippy::panic)]
pub fn sign(bytes: Vec<u8>, by: &[&Publisher]) -> Vec<u8> {
    let mut module = match wasmsign2::Module::deserialize(&mut std::io::Cursor::new(bytes)) {
        Ok(module) => module,
        Err(error) => panic!("not a module: {error}"),
    };
    for publisher in by {
        module = match publisher
            .wasmsign2_key()
            .sign_multi(module, None, false, false)
        {
            Ok((signed, _)) => signed,
            Err(error) => panic!("signing failed: {error}"),
        };
    }
    let mut out = Vec::new();
    if let Err(error) = module.serialize(&mut out) {
        panic!("serializing failed: {error}");
    }
    out
}

/// Appends a custom section.
pub fn custom_section(out: &mut Vec<u8>, name: &str, data: &[u8]) {
    let mut payload = Vec::new();
    leb128(&mut payload, name.len());
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    out.push(0);
    leb128(out, payload.len());
    out.extend_from_slice(&payload);
}

fn leb128(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}
