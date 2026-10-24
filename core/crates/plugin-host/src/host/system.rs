//! `system`: time, locale and logging.

use super::{HostState, lock, strings_fit, trap};
use crate::bindings::ristle::plugin::system::{Host, Instant, Level};
use crate::limits::LOG_MESSAGE_BYTES;

impl Host for HostState {
    fn now(&mut self) -> wasmtime::Result<Instant> {
        Ok(self.timed(|state| state.services().clock.now_ms()))
    }

    fn locale(&mut self) -> wasmtime::Result<String> {
        Ok(self.timed(|state| lock(&state.services().locale).clone()))
    }

    fn log(&mut self, level: Level, message: String) -> wasmtime::Result<()> {
        if !strings_fit(&[&message]) {
            return Err(trap("system.log: message exceeds 64 KiB"));
        }
        self.timed(|state| {
            let plugin = &state.plugin;
            let dropped = {
                let mut bucket = lock(&plugin.log);
                if !bucket.take() {
                    return;
                }
                std::mem::take(&mut bucket.dropped)
            };
            let message = cut(&message, LOG_MESSAGE_BYTES);
            let id = plugin.id.as_str();
            match level {
                Level::Debug => tracing::debug!(plugin = id, dropped, "{message}"),
                Level::Info => tracing::info!(plugin = id, dropped, "{message}"),
                Level::Warn => tracing::warn!(plugin = id, dropped, "{message}"),
                Level::Error => tracing::error!(plugin = id, dropped, "{message}"),
            }
        });
        Ok(())
    }
}

/// At most `max` bytes, cut at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::cut;

    #[test]
    fn messages_are_cut_at_character_boundaries() {
        assert_eq!(cut("abc", 5), "abc");
        assert_eq!(cut("aé", 2), "a");
        assert_eq!(cut("aé", 3), "aé");
    }
}
