//! Fixed bounds on every instance and call (`docs/plugin-api/packaging.md`,
//! "Installing"). They are the host's, never the manifest's.

use std::time::Duration;

/// Linear memory per instance: 64 MiB.
pub const MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;

/// Table elements per instance.
pub const MAX_TABLE_ELEMENTS: usize = 10_000;

/// Guest time per call under Cranelift: 2 s. Time spent waiting in a host
/// import does not count.
pub const GUEST_TIME: Duration = Duration::from_secs(2);

/// Guest time per `byte-source.read` under Cranelift: 250 ms, since playback
/// waits on it.
pub const BYTE_SOURCE_GUEST_TIME: Duration = Duration::from_millis(250);

/// How much more guest time an interpreter (Pulley, on iOS) gets.
pub const INTERPRETER_FACTOR: u32 = 10;

/// Total time per call, imports included: 60 s.
pub const CALL_TIME: Duration = Duration::from_secs(60);

/// Plugin calls running at once across all Plugins on a phone.
pub const PHONE_CONCURRENT_CALLS: usize = 4;

/// Plugin calls running at once across all Plugins on a desktop.
pub const DESKTOP_CONCURRENT_CALLS: usize = 8;

/// An instance whose linear memory grew by more than this since it was
/// instantiated is discarded after its call, since linear memory never
/// shrinks.
pub const RECYCLE_GROWTH_BYTES: usize = 8 * 1024 * 1024;

/// How long an idle instance is kept for its connection.
pub const IDLE_INSTANCE: Duration = Duration::from_secs(5 * 60);

/// Idle instances kept per connection.
pub const IDLE_PER_CONNECTION: usize = 2;

/// Log messages a Plugin may write in a burst, and how many it earns back
/// per second.
pub const LOG_BURST: u32 = 200;
/// See [`LOG_BURST`].
pub const LOG_PER_SECOND: u32 = 10;

/// The longest log message kept; longer ones are cut.
pub const LOG_MESSAGE_BYTES: usize = 4 * 1024;

/// Refuses growth beyond the instance limits, and records how large linear
/// memory became.
#[derive(Debug, Default)]
pub(crate) struct Limiter {
    /// Linear memory now, in bytes, over every memory of the instance.
    pub(crate) memory_bytes: usize,
}

impl wasmtime::ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let total = self.memory_bytes - current.min(self.memory_bytes) + desired;
        if total > MAX_MEMORY_BYTES {
            return Ok(false);
        }
        self.memory_bytes = total;
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= MAX_TABLE_ELEMENTS)
    }
}
