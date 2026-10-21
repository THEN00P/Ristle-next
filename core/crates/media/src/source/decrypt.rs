//! Host-side decryption of a progressive resource (ADR 0004, WIT
//! `decryption`): AES-CTR, AES-CBC and AES-CBC with PKCS#7, with 128, 192
//! or 256-bit keys, entered at any offset so seeks need not decrypt from the
//! start.

use aes::{Aes128, Aes192, Aes256};
use cbc::cipher::{BlockDecryptMut, KeyIvInit, StreamCipher, StreamCipherSeek};

use crate::Error;

const BLOCK: usize = 16;

/// How a progressive resource is encrypted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AesMode {
    /// Counter mode; the counter is `iv` at byte 0 and counts 128-bit
    /// big-endian.
    Ctr,
    /// CBC without padding.
    Cbc,
    /// CBC with PKCS#7 padding.
    CbcPkcs7,
}

/// Decryption parameters from the Plugin's Stream Source.
#[derive(Clone, PartialEq, Eq)]
pub struct Decryption {
    /// The mode.
    pub mode: AesMode,
    /// 16, 24 or 32 bytes.
    pub key: Vec<u8>,
    /// The IV (CBC) or initial counter block (CTR).
    pub iv: [u8; 16],
    /// Plaintext bytes before the container starts, which the host drops.
    pub skip: u32,
}

impl std::fmt::Debug for Decryption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never log key material.
        f.debug_struct("Decryption")
            .field("mode", &self.mode)
            .field("key_bits", &(self.key.len() * 8))
            .field("skip", &self.skip)
            .finish_non_exhaustive()
    }
}

impl Decryption {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if matches!(self.key.len(), 16 | 24 | 32) {
            Ok(())
        } else {
            Err(Error::UnsupportedEncryption {
                scheme: format!("AES with a {}-byte key", self.key.len()),
            })
        }
    }
}

type Ctr<C> = ctr::Ctr128BE<C>;

enum Cipher {
    Ctr128(Ctr<Aes128>),
    Ctr192(Ctr<Aes192>),
    Ctr256(Ctr<Aes256>),
    Cbc128(cbc::Decryptor<Aes128>),
    Cbc192(cbc::Decryptor<Aes192>),
    Cbc256(cbc::Decryptor<Aes256>),
}

/// Decrypts a resource from some offset on.
pub(crate) struct Decryptor {
    cipher: Option<Cipher>,
    mode: AesMode,
    key: Vec<u8>,
    /// CBC entered past the start: the first ciphertext block is the IV.
    iv_pending: Vec<u8>,
    /// Ciphertext not yet decrypted (CBC works in whole blocks; with PKCS#7
    /// the last block waits for the end).
    held: Vec<u8>,
}

impl Decryptor {
    /// A decryptor for plaintext from `offset`. Returns it and the offset
    /// ciphertext must be fetched from; plaintext output starts at
    /// `start_of_output`, which is `offset` rounded down to a block for CBC.
    pub(crate) fn at(params: &Decryption, offset: u64) -> Result<(Self, Fetched), Error> {
        params.validate()?;
        let key = params.key.clone();
        let mut decryptor = Self {
            cipher: None,
            mode: params.mode,
            key,
            iv_pending: Vec::new(),
            held: Vec::new(),
        };
        match params.mode {
            AesMode::Ctr => {
                let mut cipher = decryptor.ctr(&params.iv)?;
                match &mut cipher {
                    Cipher::Ctr128(c) => c.seek(offset),
                    Cipher::Ctr192(c) => c.seek(offset),
                    Cipher::Ctr256(c) => c.seek(offset),
                    _ => {}
                }
                decryptor.cipher = Some(cipher);
                Ok((
                    decryptor,
                    Fetched {
                        from: offset,
                        output_from: offset,
                    },
                ))
            }
            AesMode::Cbc | AesMode::CbcPkcs7 => {
                let aligned = offset / BLOCK as u64 * BLOCK as u64;
                if aligned == 0 {
                    decryptor.cipher = Some(decryptor.cbc(&params.iv)?);
                    Ok((
                        decryptor,
                        Fetched {
                            from: 0,
                            output_from: 0,
                        },
                    ))
                } else {
                    // The previous ciphertext block is this block's IV.
                    Ok((
                        decryptor,
                        Fetched {
                            from: aligned - BLOCK as u64,
                            output_from: aligned,
                        },
                    ))
                }
            }
        }
    }

    fn ctr(&self, iv: &[u8; 16]) -> Result<Cipher, Error> {
        let bad = |_| Error::UnsupportedEncryption {
            scheme: "AES key".into(),
        };
        Ok(match self.key.len() {
            16 => Cipher::Ctr128(Ctr::new_from_slices(&self.key, iv).map_err(bad)?),
            24 => Cipher::Ctr192(Ctr::new_from_slices(&self.key, iv).map_err(bad)?),
            _ => Cipher::Ctr256(Ctr::new_from_slices(&self.key, iv).map_err(bad)?),
        })
    }

    fn cbc(&self, iv: &[u8]) -> Result<Cipher, Error> {
        let bad = |_| Error::UnsupportedEncryption {
            scheme: "AES key".into(),
        };
        Ok(match self.key.len() {
            16 => Cipher::Cbc128(cbc::Decryptor::new_from_slices(&self.key, iv).map_err(bad)?),
            24 => Cipher::Cbc192(cbc::Decryptor::new_from_slices(&self.key, iv).map_err(bad)?),
            _ => Cipher::Cbc256(cbc::Decryptor::new_from_slices(&self.key, iv).map_err(bad)?),
        })
    }

    /// Decrypts the next ciphertext and appends the plaintext.
    pub(crate) fn push(&mut self, mut data: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
        if self.cipher.is_none() {
            let take = (BLOCK - self.iv_pending.len()).min(data.len());
            self.iv_pending.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.iv_pending.len() < BLOCK {
                return Ok(());
            }
            let iv = std::mem::take(&mut self.iv_pending);
            self.cipher = Some(self.cbc(&iv)?);
        }
        let Some(cipher) = &mut self.cipher else {
            return Ok(());
        };
        match cipher {
            Cipher::Ctr128(c) => apply(c, data, out),
            Cipher::Ctr192(c) => apply(c, data, out),
            Cipher::Ctr256(c) => apply(c, data, out),
            Cipher::Cbc128(_) | Cipher::Cbc192(_) | Cipher::Cbc256(_) => {
                self.held.extend_from_slice(data);
                let keep = if self.mode == AesMode::CbcPkcs7 { 1 } else { 0 };
                let ready = (self.held.len().saturating_sub(keep) / BLOCK) * BLOCK;
                let start = out.len();
                out.extend(self.held.drain(..ready));
                decrypt_blocks(cipher, &mut out[start..]);
            }
        }
        Ok(())
    }

    /// The end of the resource: removes PKCS#7 padding.
    pub(crate) fn finish(mut self, out: &mut Vec<u8>) -> Result<(), Error> {
        if self.mode != AesMode::CbcPkcs7 {
            if !self.held.is_empty() {
                tracing::warn!(
                    bytes = self.held.len(),
                    "dropping a partial CBC block at the end"
                );
            }
            return Ok(());
        }
        let Some(cipher) = &mut self.cipher else {
            return Err(Error::malformed(
                "AES-CBC resource",
                "ended inside the first block",
            ));
        };
        if self.held.len() != BLOCK {
            return Err(Error::malformed(
                "AES-CBC resource",
                "does not end on a block",
            ));
        }
        let mut block = std::mem::take(&mut self.held);
        decrypt_blocks(cipher, &mut block);
        let padding = usize::from(block[BLOCK - 1]);
        if padding == 0
            || padding > BLOCK
            || block[BLOCK - padding..]
                .iter()
                .any(|&b| usize::from(b) != padding)
        {
            return Err(Error::malformed(
                "AES-CBC resource",
                "bad PKCS#7 padding (wrong key?)",
            ));
        }
        out.extend_from_slice(&block[..BLOCK - padding]);
        Ok(())
    }
}

/// Where to fetch ciphertext from, and where the plaintext it yields starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fetched {
    pub(crate) from: u64,
    pub(crate) output_from: u64,
}

fn apply<C: StreamCipher>(cipher: &mut C, data: &[u8], out: &mut Vec<u8>) {
    let start = out.len();
    out.extend_from_slice(data);
    cipher.apply_keystream(&mut out[start..]);
}

fn decrypt_blocks(cipher: &mut Cipher, data: &mut [u8]) {
    for block in data.as_chunks_mut::<BLOCK>().0 {
        match cipher {
            Cipher::Cbc128(c) => c.decrypt_block_mut(block.into()),
            Cipher::Cbc192(c) => c.decrypt_block_mut(block.into()),
            Cipher::Cbc256(c) => c.decrypt_block_mut(block.into()),
            Cipher::Ctr128(_) | Cipher::Ctr192(_) | Cipher::Ctr256(_) => {}
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cbc::cipher::BlockEncryptMut;
    use cbc::cipher::block_padding::{NoPadding, Pkcs7};

    pub(crate) fn encrypt(params: &Decryption, plain: &[u8]) -> Vec<u8> {
        match params.mode {
            AesMode::Ctr => {
                let mut data = plain.to_vec();
                let mut c = Ctr::<Aes128>::new_from_slices(&params.key, &params.iv).unwrap();
                c.apply_keystream(&mut data);
                data
            }
            AesMode::Cbc => cbc::Encryptor::<Aes128>::new_from_slices(&params.key, &params.iv)
                .unwrap()
                .encrypt_padded_vec_mut::<NoPadding>(plain),
            AesMode::CbcPkcs7 => cbc::Encryptor::<Aes128>::new_from_slices(&params.key, &params.iv)
                .unwrap()
                .encrypt_padded_vec_mut::<Pkcs7>(plain),
        }
    }

    fn params(mode: AesMode) -> Decryption {
        Decryption {
            mode,
            key: vec![5; 16],
            iv: [7; 16],
            skip: 0,
        }
    }

    fn decrypt_from(params: &Decryption, cipher: &[u8], offset: u64, chunk: usize) -> Vec<u8> {
        let (mut decryptor, fetched) = Decryptor::at(params, offset).unwrap();
        let mut out = Vec::new();
        for piece in cipher[fetched.from as usize..].chunks(chunk) {
            decryptor.push(piece, &mut out).unwrap();
        }
        decryptor.finish(&mut out).unwrap();
        out.drain(..(offset - fetched.output_from) as usize);
        out
    }

    #[test]
    fn every_mode_decrypts_from_any_offset_in_any_chunking() {
        let plain: Vec<u8> = (0..1000u32).map(|i| (i * 31 % 251) as u8).collect();
        for mode in [AesMode::Ctr, AesMode::Cbc, AesMode::CbcPkcs7] {
            let params = params(mode);
            // CBC without padding needs whole blocks.
            let plain = if mode == AesMode::Cbc {
                &plain[..992]
            } else {
                &plain[..]
            };
            let cipher = encrypt(&params, plain);
            for offset in [0u64, 1, 15, 16, 17, 500, 975] {
                for chunk in [1, 13, 64, 4096] {
                    let out = decrypt_from(&params, &cipher, offset, chunk);
                    assert_eq!(
                        out,
                        &plain[offset as usize..],
                        "{mode:?} at {offset} by {chunk}"
                    );
                }
            }
        }
    }

    #[test]
    fn keys_of_other_lengths_are_refused_and_never_printed() {
        let mut params = params(AesMode::Ctr);
        params.key = vec![1; 20];
        assert!(matches!(
            Decryptor::at(&params, 0),
            Err(Error::UnsupportedEncryption { .. })
        ));
        let shown = format!("{params:?}");
        assert!(!shown.contains("1, 1"), "{shown}");
        params.key = vec![1; 32];
        assert!(Decryptor::at(&params, 0).is_ok());
    }
}
