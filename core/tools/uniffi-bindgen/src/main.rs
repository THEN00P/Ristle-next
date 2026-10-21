//! The UniFFI bindings generator, built from the same `uniffi` version as
//! `ristle-ffi` so its metadata always matches.
//!
//! Library mode only, from an unstripped build (ADR 0001):
//!
//! ```sh
//! cargo build -p ristle-ffi
//! cargo run -p uniffi-bindgen -- generate --library target/debug/libristle_core.so \
//!     --language kotlin --language swift --out-dir target/bindings
//! ```

fn main() {
    uniffi::uniffi_bindgen_main()
}
