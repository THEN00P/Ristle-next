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

/// How often idle instances past their time are released, whether or not
/// their Plugin is called.
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// What the host may allocate while lifting one value out of a Plugin: an
/// import's arguments or an export's result. wasmtime charges every string
/// and list against it before copying, so descriptors that point at the
/// same guest memory many times cannot make the host copy it many times.
/// It is the import bound of `host.wit`: 16 MiB of byte payloads plus 1 MiB
/// of everything else.
pub const HOSTCALL_FUEL_BYTES: usize = 17 * 1024 * 1024;

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
/// memory and tables became. Growth is charged when it is allowed and given
/// back if wasmtime then fails it.
#[derive(Debug, Default)]
pub(crate) struct Limiter {
    /// Linear memory now, in bytes, over every memory of the instance.
    pub(crate) memory_bytes: usize,
    /// Table elements now, over every table of the instance.
    pub(crate) table_elements: usize,
    /// What the counters were before the growth wasmtime is performing.
    memory_before: Option<usize>,
    tables_before: Option<usize>,
}

/// The instance-wide total after one item grows from `current` to
/// `desired`, if it fits `limit` and the item's own `maximum`.
fn grown(
    total: usize,
    current: usize,
    desired: usize,
    maximum: Option<usize>,
    limit: usize,
) -> Option<usize> {
    if maximum.is_some_and(|maximum| desired > maximum) {
        return None;
    }
    let total = total - current.min(total) + desired;
    (total <= limit).then_some(total)
}

impl wasmtime::ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        match grown(
            self.memory_bytes,
            current,
            desired,
            maximum,
            MAX_MEMORY_BYTES,
        ) {
            Some(total) => {
                self.memory_before = Some(self.memory_bytes);
                self.memory_bytes = total;
                Ok(true)
            }
            None => {
                self.memory_before = None;
                Ok(false)
            }
        }
    }

    fn memory_grow_failed(&mut self, _error: wasmtime::Error) -> wasmtime::Result<()> {
        if let Some(before) = self.memory_before.take() {
            self.memory_bytes = before;
        }
        Ok(())
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        match grown(
            self.table_elements,
            current,
            desired,
            maximum,
            MAX_TABLE_ELEMENTS,
        ) {
            Some(total) => {
                self.tables_before = Some(self.table_elements);
                self.table_elements = total;
                Ok(true)
            }
            None => {
                self.tables_before = None;
                Ok(false)
            }
        }
    }

    fn table_grow_failed(&mut self, _error: wasmtime::Error) -> wasmtime::Result<()> {
        if let Some(before) = self.tables_before.take() {
            self.table_elements = before;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use wasmtime::ResourceLimiter;

    use super::*;

    #[test]
    fn limits_count_every_memory_and_table_of_an_instance() {
        let mut limiter = Limiter::default();
        // Two tables of 6 000 elements are 12 000 in the instance.
        assert!(limiter.table_growing(0, 6_000, None).unwrap());
        assert!(!limiter.table_growing(0, 6_000, None).unwrap());
        assert!(limiter.table_growing(0, 4_000, None).unwrap());
        assert!(!limiter.table_growing(4_000, 4_001, None).unwrap());
        assert_eq!(limiter.table_elements, MAX_TABLE_ELEMENTS);
        // Memories likewise.
        let half = MAX_MEMORY_BYTES / 2;
        assert!(limiter.memory_growing(0, half, None).unwrap());
        assert!(limiter.memory_growing(0, half, None).unwrap());
        assert!(!limiter.memory_growing(0, 1, None).unwrap());
    }

    #[test]
    fn refused_or_failed_growth_costs_nothing() {
        let mib = 1024 * 1024;
        let mut limiter = Limiter::default();
        assert!(limiter.memory_growing(0, mib, Some(2 * mib)).unwrap());
        // Beyond the memory's own maximum: refused, nothing charged.
        assert!(
            !limiter
                .memory_growing(mib, 64 * mib, Some(2 * mib))
                .unwrap()
        );
        assert_eq!(limiter.memory_bytes, mib);
        // Allowed, but wasmtime fails it: given back.
        assert!(limiter.memory_growing(mib, 2 * mib, Some(2 * mib)).unwrap());
        limiter
            .memory_grow_failed(wasmtime::Error::msg("mmap failed"))
            .unwrap();
        assert_eq!(limiter.memory_bytes, mib);
        // A refusal never rolls back an earlier success.
        assert!(limiter.memory_growing(mib, 2 * mib, None).unwrap());
        assert!(!limiter.memory_growing(2 * mib, 100 * mib, None).unwrap());
        limiter
            .memory_grow_failed(wasmtime::Error::msg("refused"))
            .unwrap();
        assert_eq!(limiter.memory_bytes, 2 * mib);
        // Tables alike.
        assert!(!limiter.table_growing(0, 20, Some(10)).unwrap());
        assert_eq!(limiter.table_elements, 0);
        assert!(limiter.table_growing(0, 10, Some(10)).unwrap());
        limiter
            .table_grow_failed(wasmtime::Error::msg("failed"))
            .unwrap();
        assert_eq!(limiter.table_elements, 0);
    }
}
