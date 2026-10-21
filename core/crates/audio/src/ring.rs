//! A single-producer, single-consumer PCM ring whose read index the producer
//! side may also advance.
//!
//! Positions are absolute frame counts that only grow, so the engine can name
//! a point in a slot's audio ("the Track ends at frame 48 000") without
//! tracking wrap-around.
//!
//! Discarding is what spike B got wrong: it replaced the whole ring on a seek
//! and handed the new consumer to the render callback through a bounded queue,
//! which the callback does not drain while output is paused, so a third seek
//! while paused lost the ring. Here a seek never touches ring ownership. The
//! control side raises the read index past the stale audio with `fetch_max`,
//! paused or not, and the consumer commits each read with a compare-exchange.
//! When the index moved underneath it, the consumer drops what it copied.
//! Samples are stored as `AtomicU32` bit patterns with relaxed ordering, so a
//! read racing a discard is not a data race and needs no `unsafe`.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub(crate) struct Ring {
    samples: Box<[AtomicU32]>,
    capacity: u64,
    channels: usize,
    read: AtomicU64,
    write: AtomicU64,
}

impl Ring {
    /// A ring for `capacity` frames of `channels` samples. Both are at least 1.
    pub(crate) fn new(capacity: usize, channels: usize) -> Self {
        let capacity = capacity.max(1);
        let channels = channels.max(1);
        let samples = (0..capacity * channels)
            .map(|_| AtomicU32::new(0))
            .collect();
        Self {
            samples,
            capacity: capacity as u64,
            channels,
            read: AtomicU64::new(0),
            write: AtomicU64::new(0),
        }
    }

    pub(crate) fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Frames read or discarded so far.
    pub(crate) fn read_position(&self) -> u64 {
        self.read.load(Ordering::Acquire)
    }

    /// Frames written so far.
    pub(crate) fn write_position(&self) -> u64 {
        self.write.load(Ordering::Acquire)
    }

    /// Frames waiting to be read.
    pub(crate) fn readable(&self) -> u64 {
        let write = self.write.load(Ordering::Acquire);
        write.saturating_sub(self.read.load(Ordering::Acquire))
    }

    /// Frames the producer can write now.
    pub(crate) fn writable(&self) -> u64 {
        self.capacity - self.readable().min(self.capacity)
    }

    /// Producer: appends whole frames from `samples` and returns how many
    /// frames fit.
    pub(crate) fn write(&self, samples: &[f32]) -> usize {
        let write = self.write.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        let free = self.capacity - (write - read).min(self.capacity);
        let frames = (samples.len() / self.channels).min(free as usize);
        for (offset, frame) in samples.chunks_exact(self.channels).take(frames).enumerate() {
            let base = self.slot(write + offset as u64);
            for (channel, sample) in frame.iter().enumerate() {
                self.samples[base + channel].store(sample.to_bits(), Ordering::Relaxed);
            }
        }
        self.write.store(write + frames as u64, Ordering::Release);
        frames
    }

    /// Consumer: copies up to `limit` frames (and at most `out.len()`) into
    /// `out` and returns the count. Real-time safe: no locks, no allocation.
    pub(crate) fn read_into(&self, out: &mut [f32], limit: u64) -> usize {
        let wanted = (out.len() / self.channels) as u64;
        loop {
            let read = self.read.load(Ordering::Acquire);
            let write = self.write.load(Ordering::Acquire);
            let frames = write.saturating_sub(read).min(limit).min(wanted) as usize;
            if frames == 0 {
                return 0;
            }
            for (offset, frame) in out.chunks_exact_mut(self.channels).take(frames).enumerate() {
                let base = self.slot(read + offset as u64);
                for (channel, sample) in frame.iter_mut().enumerate() {
                    *sample = f32::from_bits(self.samples[base + channel].load(Ordering::Relaxed));
                }
            }
            if self
                .read
                .compare_exchange(
                    read,
                    read + frames as u64,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return frames;
            }
            // A discard moved the read index while we copied: what we copied
            // is stale, possibly overwritten. Start over from the new index.
            out[..frames * self.channels].fill(0.0);
        }
    }

    /// Either side: drops everything before `position`. Only ever moves the
    /// read index forward, and never past what was written.
    pub(crate) fn discard_to(&self, position: u64) {
        let position = position.min(self.write.load(Ordering::Acquire));
        self.read.fetch_max(position, Ordering::AcqRel);
    }

    fn slot(&self, position: u64) -> usize {
        (position % self.capacity) as usize * self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_keeps_frames_whole() {
        let ring = Ring::new(4, 2);
        assert_eq!(ring.write(&[1.0, 1.0, 2.0, 2.0, 3.0, 3.0]), 3);
        let mut out = [0.0; 4];
        assert_eq!(ring.read_into(&mut out, u64::MAX), 2);
        assert_eq!(out, [1.0, 1.0, 2.0, 2.0]);
        assert_eq!(ring.write(&[4.0, 4.0, 5.0, 5.0, 6.0, 6.0, 7.0, 7.0]), 3);
        let mut out = [0.0; 8];
        assert_eq!(ring.read_into(&mut out, u64::MAX), 4);
        assert_eq!(out, [3.0, 3.0, 4.0, 4.0, 5.0, 5.0, 6.0, 6.0]);
        assert_eq!((ring.read_position(), ring.write_position()), (6, 6));
    }

    #[test]
    fn read_stops_at_the_limit() {
        let ring = Ring::new(8, 1);
        ring.write(&[1.0, 2.0, 3.0]);
        let mut out = [0.0; 3];
        assert_eq!(ring.read_into(&mut out, 2), 2);
        assert_eq!(out, [1.0, 2.0, 0.0]);
    }

    #[test]
    fn discarding_a_full_ring_frees_it_without_the_consumer() {
        let ring = Ring::new(4, 1);
        assert_eq!(ring.write(&[1.0; 6]), 4);
        assert_eq!(ring.writable(), 0);
        ring.discard_to(ring.write_position());
        ring.discard_to(ring.write_position());
        ring.discard_to(2);
        assert_eq!(ring.readable(), 0);
        assert_eq!(ring.write(&[9.0, 8.0]), 2);
        let mut out = [0.0; 4];
        assert_eq!(ring.read_into(&mut out, u64::MAX), 2);
        assert_eq!(out[..2], [9.0, 8.0]);
    }

    #[test]
    fn discard_never_passes_the_write_index() {
        let ring = Ring::new(4, 1);
        ring.write(&[1.0]);
        ring.discard_to(10);
        assert_eq!(ring.read_position(), 1);
        assert_eq!(ring.writable(), 4);
    }
}
