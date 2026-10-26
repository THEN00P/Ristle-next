//! Random access to one file through bounded reads, the shape of the host's
//! `files.read` (at most 1 MiB per call).
//!
//! Tag parsers want `Read + Seek`. `Reader` provides it over any
//! [`RandomAccess`], fetching aligned blocks on demand, so reading the tags
//! of a large file costs a few calls near its start and end, never the
//! audio in between. A per-file budget stops a malformed file from making
//! the parser walk all of it.

use std::io::{self, Read, Seek, SeekFrom};

/// The most one `files.read` call returns.
pub const MAX_READ: u32 = 1 << 20;

/// Why a read failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// The folder is no longer readable (the host's `unauthorized`): the user
    /// must choose it again.
    #[error("the folder is no longer accessible")]
    Revoked,
    /// This file or folder cannot be read now (the host's `unavailable`).
    #[error("unavailable: {0}")]
    Unavailable(String),
}

/// A file read by offset.
pub trait RandomAccess {
    /// The file's size in bytes.
    fn size(&self) -> u64;

    /// Up to `length` bytes from `offset`, `length` at most [`MAX_READ`].
    /// Fewer only at the end of the file.
    fn read_at(&self, offset: u64, length: u32) -> Result<Vec<u8>, SourceError>;
}

impl RandomAccess for [u8] {
    fn size(&self) -> u64 {
        self.len() as u64
    }

    fn read_at(&self, offset: u64, length: u32) -> Result<Vec<u8>, SourceError> {
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.len());
        let end = start.saturating_add(length as usize).min(self.len());
        Ok(self[start..end].to_vec())
    }
}

/// Bytes fetched per call. Small enough that reading tags at both ends of a
/// file stays cheap, large enough that a 300 KiB embedded cover is a handful
/// of calls.
const BLOCK: u64 = 64 * 1024;

/// Blocks kept. Parsers go back and forth between a file's head and tail.
const CACHED_BLOCKS: usize = 4;

/// `Read + Seek` over a [`RandomAccess`], with a few cached blocks.
pub(crate) struct Reader<'a, S: RandomAccess + ?Sized> {
    source: &'a S,
    size: u64,
    position: u64,
    /// `(start, bytes)`, least recently used first.
    blocks: Vec<(u64, Vec<u8>)>,
    /// Bytes still allowed to be fetched.
    budget: u64,
    /// The first source error, kept so the caller can tell a revoked folder
    /// from a malformed file once the parser gives up.
    error: Option<SourceError>,
}

impl<'a, S: RandomAccess + ?Sized> Reader<'a, S> {
    pub(crate) fn new(source: &'a S, budget: u64) -> Self {
        Self {
            size: source.size(),
            source,
            position: 0,
            blocks: Vec::new(),
            budget,
            error: None,
        }
    }

    pub(crate) fn take_error(&mut self) -> Option<SourceError> {
        self.error.take()
    }

    /// The cached block holding `self.position`, fetched if needed, moved
    /// to the most recently used end.
    fn block(&mut self) -> io::Result<&[u8]> {
        let start = self.position - self.position % BLOCK;
        if let Some(index) = self.blocks.iter().position(|(at, _)| *at == start) {
            let block = self.blocks.remove(index);
            self.blocks.push(block);
        } else {
            let length = BLOCK.min(self.size - start);
            if length > self.budget {
                return Err(io::Error::other("read budget exhausted"));
            }
            self.budget -= length;
            // BLOCK is below MAX_READ, so the cast cannot truncate.
            let bytes = match self.source.read_at(start, length as u32) {
                Ok(bytes) => bytes,
                Err(error) => {
                    let message = error.to_string();
                    self.error.get_or_insert(error);
                    return Err(io::Error::other(message));
                }
            };
            if self.blocks.len() == CACHED_BLOCKS {
                self.blocks.remove(0);
            }
            self.blocks.push((start, bytes));
        }
        let (start, bytes) = self
            .blocks
            .last()
            .map_or((0, &[][..]), |(at, b)| (*at, b.as_slice()));
        let offset = usize::try_from(self.position - start).unwrap_or(usize::MAX);
        Ok(bytes.get(offset..).unwrap_or_default())
    }
}

impl<S: RandomAccess + ?Sized> Read for Reader<'_, S> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.position >= self.size {
            return Ok(0);
        }
        // Empty only when the source returned less than the size promised.
        let available = self.block()?;
        let n = available.len().min(out.len());
        out[..n].copy_from_slice(&available[..n]);
        self.position += n as u64;
        Ok(n)
    }
}

impl<S: RandomAccess + ?Sized> Seek for Reader<'_, S> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(delta) => self.size.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        };
        match target {
            Some(position) => {
                self.position = position;
                Ok(position)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the file",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// Records every read.
    struct Counting<'a> {
        bytes: &'a [u8],
        reads: RefCell<Vec<(u64, u32)>>,
    }

    impl RandomAccess for Counting<'_> {
        fn size(&self) -> u64 {
            self.bytes.len() as u64
        }
        fn read_at(&self, offset: u64, length: u32) -> Result<Vec<u8>, SourceError> {
            assert!(length <= MAX_READ);
            self.reads.borrow_mut().push((offset, length));
            self.bytes.read_at(offset, length)
        }
    }

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn reads_only_the_blocks_it_touches() {
        let data = bytes(1_000_000);
        let source = Counting {
            bytes: &data,
            reads: RefCell::default(),
        };
        let mut reader = Reader::new(&source, u64::MAX);

        let mut head = [0; 10];
        reader.read_exact(&mut head).unwrap();
        reader.seek(SeekFrom::End(-128)).unwrap();
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).unwrap();

        assert_eq!(head, data[..10]);
        assert_eq!(tail, data[data.len() - 128..]);
        assert_eq!(
            *source.reads.borrow(),
            [
                (0, BLOCK as u32),
                (15 * BLOCK, (1_000_000 - 15 * BLOCK) as u32)
            ]
        );

        // Back to the head: still cached.
        reader.seek(SeekFrom::Start(5)).unwrap();
        reader.read_exact(&mut head).unwrap();
        assert_eq!(head, data[5..15]);
        assert_eq!(source.reads.borrow().len(), 2);
    }

    #[test]
    fn reads_across_block_boundaries() {
        let data = bytes(3 * BLOCK as usize + 17);
        let mut reader = Reader::new(data.as_slice(), u64::MAX);
        reader.seek(SeekFrom::Start(BLOCK - 3)).unwrap();
        let mut out = vec![0; BLOCK as usize + 6];
        reader.read_exact(&mut out).unwrap();
        assert_eq!(out, data[BLOCK as usize - 3..2 * BLOCK as usize + 3]);
    }

    #[test]
    fn stops_at_the_budget() {
        let data = bytes(4 * BLOCK as usize);
        let mut reader = Reader::new(data.as_slice(), 2 * BLOCK);
        let mut out = Vec::new();
        assert!(reader.read_to_end(&mut out).is_err());
        assert_eq!(out.len(), 2 * BLOCK as usize);
    }

    struct Revoked;

    impl RandomAccess for Revoked {
        fn size(&self) -> u64 {
            100
        }
        fn read_at(&self, _: u64, _: u32) -> Result<Vec<u8>, SourceError> {
            Err(SourceError::Revoked)
        }
    }

    #[test]
    fn keeps_the_source_error() {
        let mut reader = Reader::new(&Revoked, u64::MAX);
        assert!(reader.read(&mut [0; 4]).is_err());
        assert_eq!(reader.take_error(), Some(SourceError::Revoked));
    }

    #[test]
    fn refuses_to_seek_before_the_start() {
        let data = bytes(10);
        let mut reader = Reader::new(data.as_slice(), u64::MAX);
        assert!(reader.seek(SeekFrom::Current(-1)).is_err());
    }
}
