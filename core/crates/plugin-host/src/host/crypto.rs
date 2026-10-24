//! `crypto`: done natively, so interpreted Plugins (iOS) do not pay for it.

use aes::cipher::block_padding::{NoPadding, Pkcs7};
use aes::cipher::{
    BlockDecrypt, BlockDecryptMut, BlockEncrypt, BlockEncryptMut, KeyInit, KeyIvInit, StreamCipher,
};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Digest;

use super::{HostState, MAX_PAYLOAD, too_large, trap};
use crate::bindings::ristle::plugin::crypto::{Base64, Hash, Host};
use crate::bindings::ristle::plugin::types::{AesMode, Error};

/// The most random bytes one call returns.
const MAX_RANDOM: u32 = 1024;

impl Host for HostState {
    fn aes_encrypt(
        &mut self,
        mode: AesMode,
        key: Vec<u8>,
        iv: Vec<u8>,
        data: Vec<u8>,
    ) -> wasmtime::Result<Result<Vec<u8>, Error>> {
        if key.len() + iv.len() + data.len() > MAX_PAYLOAD {
            return Ok(Err(too_large("crypto payload")));
        }
        Ok(self.timed(|_| aes(Direction::Encrypt, mode, &key, &iv, data)))
    }

    fn aes_decrypt(
        &mut self,
        mode: AesMode,
        key: Vec<u8>,
        iv: Vec<u8>,
        data: Vec<u8>,
    ) -> wasmtime::Result<Result<Vec<u8>, Error>> {
        if key.len() + iv.len() + data.len() > MAX_PAYLOAD {
            return Ok(Err(too_large("crypto payload")));
        }
        Ok(self.timed(|_| aes(Direction::Decrypt, mode, &key, &iv, data)))
    }

    fn digest(&mut self, hash: Hash, data: Vec<u8>) -> wasmtime::Result<Vec<u8>> {
        if data.len() > MAX_PAYLOAD {
            return Err(trap("crypto.digest: data exceeds 16 MiB"));
        }
        Ok(self.timed(|_| digest(hash, &data)))
    }

    fn hmac(&mut self, hash: Hash, key: Vec<u8>, data: Vec<u8>) -> wasmtime::Result<Vec<u8>> {
        if key.len() + data.len() > MAX_PAYLOAD {
            return Err(trap("crypto.hmac: payload exceeds 16 MiB"));
        }
        Ok(self.timed(|_| hmac(hash, &key, &data)))
    }

    fn base64_encode(&mut self, alphabet: Base64, data: Vec<u8>) -> wasmtime::Result<String> {
        // The encoding must itself fit a string the Plugin can receive back
        // within the payload bound.
        if data.len() > MAX_PAYLOAD / 4 * 3 {
            return Err(trap("crypto.base64-encode: data exceeds the bound"));
        }
        Ok(self.timed(|_| engine(alphabet).encode(&data)))
    }

    fn base64_decode(
        &mut self,
        alphabet: Base64,
        text: String,
    ) -> wasmtime::Result<Result<Vec<u8>, Error>> {
        // Base64 text is a byte payload more than a name, so it gets the
        // payload bound, not the 64 KiB string bound.
        if text.len() > MAX_PAYLOAD {
            return Ok(Err(too_large("base64 text")));
        }
        Ok(self.timed(|_| {
            engine(alphabet)
                .decode(text.as_bytes())
                .map_err(|error| Error::Internal(format!("base64: {error}")))
        }))
    }

    fn random(&mut self, length: u32) -> wasmtime::Result<Result<Vec<u8>, Error>> {
        if length > MAX_RANDOM {
            return Ok(Err(Error::Internal(format!(
                "at most {MAX_RANDOM} random bytes per call"
            ))));
        }
        Ok(self.timed(|_| {
            let mut bytes = vec![0u8; length as usize];
            getrandom::fill(&mut bytes)
                .map(|()| bytes)
                .map_err(|error| Error::Internal(format!("random: {error}")))
        }))
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Encrypt,
    Decrypt,
}

fn engine(alphabet: Base64) -> base64::engine::GeneralPurpose {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    match alphabet {
        Base64::Standard => STANDARD,
        Base64::StandardUnpadded => STANDARD_NO_PAD,
        Base64::Url => URL_SAFE,
        Base64::UrlUnpadded => URL_SAFE_NO_PAD,
    }
}

fn digest(hash: Hash, data: &[u8]) -> Vec<u8> {
    match hash {
        Hash::Md5 => md5::Md5::digest(data).to_vec(),
        Hash::Sha1 => sha1::Sha1::digest(data).to_vec(),
        Hash::Sha256 => sha2::Sha256::digest(data).to_vec(),
        Hash::Sha512 => sha2::Sha512::digest(data).to_vec(),
    }
}

fn hmac(hash: Hash, key: &[u8], data: &[u8]) -> Vec<u8> {
    fn run<M: Mac + KeyInit>(key: &[u8], data: &[u8]) -> Vec<u8> {
        // HMAC accepts keys of any length.
        let Ok(mut mac) = <M as KeyInit>::new_from_slice(key) else {
            return Vec::new();
        };
        mac.update(data);
        mac.finalize().into_bytes().to_vec()
    }
    match hash {
        Hash::Md5 => run::<Hmac<md5::Md5>>(key, data),
        Hash::Sha1 => run::<Hmac<sha1::Sha1>>(key, data),
        Hash::Sha256 => run::<Hmac<sha2::Sha256>>(key, data),
        Hash::Sha512 => run::<Hmac<sha2::Sha512>>(key, data),
    }
}

/// AES-128, -192 or -256 by key length.
fn aes(
    direction: Direction,
    mode: AesMode,
    key: &[u8],
    iv: &[u8],
    data: Vec<u8>,
) -> Result<Vec<u8>, Error> {
    match key.len() {
        16 => with_cipher::<aes::Aes128>(direction, mode, key, iv, data),
        24 => with_cipher::<aes::Aes192>(direction, mode, key, iv, data),
        32 => with_cipher::<aes::Aes256>(direction, mode, key, iv, data),
        other => Err(Error::Internal(format!(
            "AES keys are 16, 24 or 32 bytes, not {other}"
        ))),
    }
}

fn with_cipher<C>(
    direction: Direction,
    mode: AesMode,
    key: &[u8],
    iv: &[u8],
    mut data: Vec<u8>,
) -> Result<Vec<u8>, Error>
where
    C: BlockEncrypt
        + BlockDecrypt
        + KeyInit
        + aes::cipher::BlockCipher
        + aes::cipher::BlockSizeUser<BlockSize = aes::cipher::consts::U16>
        + Clone,
{
    let bad_iv = || Error::Internal("AES needs a 16-byte IV for this mode".to_owned());
    let unaligned = || Error::Internal("data is not a multiple of 16 bytes".to_owned());
    match mode {
        AesMode::Ctr => {
            let mut cipher = ctr::Ctr128BE::<C>::new_from_slices(key, iv).map_err(|_| bad_iv())?;
            cipher.apply_keystream(&mut data);
            Ok(data)
        }
        AesMode::Cbc => {
            if !data.len().is_multiple_of(16) {
                return Err(unaligned());
            }
            let length = data.len();
            match direction {
                Direction::Encrypt => cbc::Encryptor::<C>::new_from_slices(key, iv)
                    .map_err(|_| bad_iv())?
                    .encrypt_padded_mut::<NoPadding>(&mut data, length)
                    .map(<[u8]>::to_vec)
                    .map_err(|_| unaligned()),
                Direction::Decrypt => cbc::Decryptor::<C>::new_from_slices(key, iv)
                    .map_err(|_| bad_iv())?
                    .decrypt_padded_mut::<NoPadding>(&mut data)
                    .map(<[u8]>::to_vec)
                    .map_err(|_| unaligned()),
            }
        }
        AesMode::CbcPkcs7 => match direction {
            Direction::Encrypt => {
                let length = data.len();
                data.resize(length + 16 - length % 16, 0);
                cbc::Encryptor::<C>::new_from_slices(key, iv)
                    .map_err(|_| bad_iv())?
                    .encrypt_padded_mut::<Pkcs7>(&mut data, length)
                    .map(<[u8]>::to_vec)
                    .map_err(|_| unaligned())
            }
            Direction::Decrypt => cbc::Decryptor::<C>::new_from_slices(key, iv)
                .map_err(|_| bad_iv())?
                .decrypt_padded_mut::<Pkcs7>(&mut data)
                .map(<[u8]>::to_vec)
                .map_err(|_| Error::Internal("bad PKCS#7 padding".to_owned())),
        },
        AesMode::Ecb => {
            if !data.len().is_multiple_of(16) {
                return Err(unaligned());
            }
            if !iv.is_empty() {
                return Err(Error::Internal("ECB takes no IV".to_owned()));
            }
            let cipher = C::new_from_slice(key).map_err(|_| bad_iv())?;
            for block in data.as_chunks_mut::<16>().0 {
                let block = aes::Block::from_mut_slice(block.as_mut_slice());
                match direction {
                    Direction::Encrypt => cipher.encrypt_block(block),
                    Direction::Decrypt => cipher.decrypt_block(block),
                }
            }
            Ok(data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn digests_and_hmacs_match_known_answers() {
        assert_eq!(
            hex(&digest(Hash::Md5, b"abc")),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hex(&digest(Hash::Sha1, b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&digest(Hash::Sha256, b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            &hex(&digest(Hash::Sha512, b"abc"))[..16],
            "ddaf35a193617aba"
        );
        // RFC 4231, test case 2.
        assert_eq!(
            hex(&hmac(
                Hash::Sha256,
                b"Jefe",
                b"what do ya want for nothing?"
            )),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn aes_matches_nist_vectors_and_round_trips() {
        // NIST SP 800-38A, F.5.1 (CTR-AES128) and F.2.1 (CBC-AES128).
        let key = unhex("2b7e151628aed2a6abf7158809cf4f3c");
        let plain = unhex("6bc1bee22e409f96e93d7e117393172a");
        let ctr_iv = unhex("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
        let ctr = aes(
            Direction::Encrypt,
            AesMode::Ctr,
            &key,
            &ctr_iv,
            plain.clone(),
        )
        .unwrap();
        assert_eq!(hex(&ctr), "874d6191b620e3261bef6864990db6ce");
        let cbc_iv = unhex("000102030405060708090a0b0c0d0e0f");
        let cbc = aes(
            Direction::Encrypt,
            AesMode::Cbc,
            &key,
            &cbc_iv,
            plain.clone(),
        )
        .unwrap();
        assert_eq!(hex(&cbc), "7649abac8119b246cee98e9b12e9197d");
        assert_eq!(
            aes(Direction::Decrypt, AesMode::Cbc, &key, &cbc_iv, cbc).unwrap(),
            plain
        );
        // ECB of the same block (F.1.1).
        let ecb = aes(Direction::Encrypt, AesMode::Ecb, &key, &[], plain.clone()).unwrap();
        assert_eq!(hex(&ecb), "3ad77bb40d7a3660a89ecaf32466ef97");
        // PKCS#7 pads to the next block and strips again.
        let padded = aes(
            Direction::Encrypt,
            AesMode::CbcPkcs7,
            &key,
            &cbc_iv,
            b"hello".to_vec(),
        )
        .unwrap();
        assert_eq!(padded.len(), 16);
        assert_eq!(
            aes(Direction::Decrypt, AesMode::CbcPkcs7, &key, &cbc_iv, padded).unwrap(),
            b"hello"
        );
        // 256-bit keys work too.
        let key256 = vec![7u8; 32];
        let round = aes(
            Direction::Encrypt,
            AesMode::Ctr,
            &key256,
            &ctr_iv,
            plain.clone(),
        )
        .unwrap();
        assert_eq!(
            aes(Direction::Decrypt, AesMode::Ctr, &key256, &ctr_iv, round).unwrap(),
            plain
        );
    }

    #[test]
    fn bad_keys_ivs_and_lengths_are_errors() {
        let plain = vec![0u8; 16];
        assert!(
            aes(
                Direction::Encrypt,
                AesMode::Ctr,
                &[0; 15],
                &[0; 16],
                plain.clone()
            )
            .is_err()
        );
        assert!(
            aes(
                Direction::Encrypt,
                AesMode::Cbc,
                &[0; 16],
                &[0; 8],
                plain.clone()
            )
            .is_err()
        );
        assert!(
            aes(
                Direction::Encrypt,
                AesMode::Cbc,
                &[0; 16],
                &[0; 16],
                vec![0; 15]
            )
            .is_err()
        );
        assert!(
            aes(
                Direction::Decrypt,
                AesMode::CbcPkcs7,
                &[0; 16],
                &[0; 16],
                plain
            )
            .is_err()
        );
    }
}
