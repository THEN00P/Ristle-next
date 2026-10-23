//! The Local Files Plugin (CONTEXT.md, Local Track): music in folders the
//! user granted to one Provider Connection (a Folder Grant).
//!
//! It is a Plugin like any other (ADR 0003, ADR 0010) and reaches files only
//! through the host's `files` interface. It reports every audio file as its
//! own Track, with its format and bitrate; the core fingerprints the decoded
//! audio, groups files into Local Tracks and plays the best one (ADR 0004).
//!
//! This crate is plain Rust over two small traits, [`RandomAccess`] for one
//! file and [`scan::Folders`] for directory listings, so it is tested
//! natively; the component glue maps them onto the WIT imports.

pub mod probe;
pub mod scan;
pub mod source;

pub use crate::probe::{Date, Format, Metadata, ProbeError, probe};
pub use crate::source::{MAX_READ, RandomAccess, SourceError};
