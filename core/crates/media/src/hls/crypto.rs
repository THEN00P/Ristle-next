//! HLS AES-128: CBC with PKCS#7 padding over a whole segment (or init
//! section), decrypted while it downloads.

use aes::Aes128;
use cbc::cipher::{BlockDecryptMut, KeyIvInit};

use crate::Error;

const BLOCK: usize = 16;

/// Decrypts a CBC stream fed in arbitrary chunks. The last block is held
/// back until the end, because it carries the padding.
pub(crate) struct CbcStream {
    cipher: cbc::Decryptor<Aes128>,
    held: Vec<u8>,
}

impl CbcStream {
    pub(crate) fn new(key: &[u8; 16], iv: &[u8; 16]) -> Self {
        Self {
            cipher: cbc::Decryptor::<Aes128>::new(key.into(), iv.into()),
            held: Vec::new(),
        }
    }

    /// Decrypts what can be decrypted and appends the plaintext to `out`.
    pub(crate) fn push(&mut self, data: &[u8], out: &mut Vec<u8>) {
        self.held.extend_from_slice(data);
        // Keep at least one whole block back.
        let ready = (self.held.len().saturating_sub(1) / BLOCK) * BLOCK;
        let start = out.len();
        out.extend(self.held.drain(..ready));
        for block in out[start..].as_chunks_mut::<BLOCK>().0 {
            self.cipher.decrypt_block_mut(block.into());
        }
    }

    /// The end of the ciphertext: decrypts the last block and removes the
    /// padding.
    pub(crate) fn finish(mut self, out: &mut Vec<u8>) -> Result<(), Error> {
        if self.held.len() != BLOCK {
            return Err(Error::malformed(
                "AES-128 segment",
                format!(
                    "ciphertext ends {} bytes into a block",
                    self.held.len() % BLOCK
                ),
            ));
        }
        let mut block = [0u8; BLOCK];
        block.copy_from_slice(&self.held);
        self.cipher.decrypt_block_mut((&mut block).into());
        let padding = usize::from(block[BLOCK - 1]);
        if padding == 0
            || padding > BLOCK
            || block[BLOCK - padding..]
                .iter()
                .any(|&b| usize::from(b) != padding)
        {
            return Err(Error::malformed(
                "AES-128 segment",
                "bad PKCS#7 padding (wrong key?)",
            ));
        }
        out.extend_from_slice(&block[..BLOCK - padding]);
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cbc::cipher::BlockEncryptMut;
    use cbc::cipher::block_padding::Pkcs7;

    pub(crate) fn encrypt(plain: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
        cbc::Encryptor::<Aes128>::new(key.into(), iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain)
    }

    #[test]
    fn round_trips_aes_128_cbc() {
        let key = [7_u8; 16];
        let iv = crate::hls::playlist::sequence_iv(3);
        let plain = b"sixteen byte blk plus some extra".to_vec();
        let encrypted = encrypt(&plain, &key, &iv);
        let mut decrypted = Vec::new();
        let mut stream = CbcStream::new(&key, &iv);
        stream.push(&encrypted, &mut decrypted);
        stream.finish(&mut decrypted).unwrap();
        assert_eq!(decrypted, plain);
    }

    #[test]
    fn decrypts_in_any_chunking() {
        let key = [3u8; 16];
        let iv = [9u8; 16];
        let plain: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
        let encrypted = encrypt(&plain, &key, &iv);
        for chunk in [1, 15, 16, 17, 100, encrypted.len()] {
            let mut out = Vec::new();
            let mut stream = CbcStream::new(&key, &iv);
            for piece in encrypted.chunks(chunk) {
                stream.push(piece, &mut out);
            }
            stream.finish(&mut out).unwrap();
            assert_eq!(out, plain, "chunk {chunk}");
        }
    }

    #[test]
    fn a_wrong_key_or_truncated_ciphertext_fails() {
        let encrypted = encrypt(b"some plaintext", &[1; 16], &[0; 16]);
        let mut out = Vec::new();
        let mut stream = CbcStream::new(&[2; 16], &[0; 16]);
        stream.push(&encrypted, &mut out);
        assert!(stream.finish(&mut out).is_err());
        let mut stream = CbcStream::new(&[1; 16], &[0; 16]);
        stream.push(&encrypted[..10], &mut out);
        assert!(stream.finish(&mut out).is_err());
    }
}
