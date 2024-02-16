#![warn(clippy::disallowed_types)]
#![warn(missing_docs)]
// https://stackoverflow.com/questions/61417452/how-to-get-a-feature-requirement-tag-in-the-documentation-generated-by-cargo-do
#![cfg_attr(all(doc, CHANNEL_NIGHTLY), feature(doc_auto_cfg))]

//! bitbazaar - An assortment of publicly available cross-language utilities useful to my projects.

mod prelude;

#[cfg(feature = "cli")]
/// Command line interface utilities.
pub mod cli;

/// Error handling utilities.
pub mod errors;
/// Hashing utilities.
pub mod hash;
/// Logging utilities
pub mod log;
/// Completely miscellaneous utilities
pub mod misc;
#[cfg(feature = "redis")]
/// Redis utilities
pub mod redis;
/// Timing utilities
pub mod timing;
