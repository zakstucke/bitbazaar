#![allow(clippy::module_inception)]
#![allow(clippy::type_complexity)]
#![warn(clippy::disallowed_types)]
#![warn(missing_docs)]

//! bitbazaar - An assortment of publicly available cross-language utilities useful to my projects.

// When docs auto created for docs.rs, will include features, given docs.rs uses nightly by default:
#![cfg_attr(all(doc, CHANNEL_NIGHTLY), feature(doc_auto_cfg))]

mod prelude;

#[cfg(feature = "cli")]
/// Command line interface utilities.
pub mod cli;
/// OS Command related utilities. Much lighter weight than cli module (which will probably be deprecated as badly written and not maintainable).
/// Not applicable to wasm
#[cfg(not(target_arch = "wasm32"))]
pub mod command;

/// Chrono utilities
pub mod chrono;
#[cfg(any(feature = "cookies_ssr", feature = "cookies_wasm"))]
/// Setting/getting cookies in wasm or ssr.
pub mod cookies;
/// Hashing & encryption utilities. Most inside is feature gated.
pub mod crypto;
/// Error handling utilities.
pub mod errors;

/// File related utilities. Therefore disabled on wasm.
#[cfg(not(target_arch = "wasm32"))]
pub mod file;
/// Logging utilities
pub mod log;
/// Completely miscellaneous utilities
pub mod misc;
#[cfg(feature = "redis")]
/// Redis utilities
pub mod redis;
#[cfg(feature = "test")]
/// Useful and reusable testing utilities.
/// Should only be enabled as a feature in dev-dependencies.
pub mod test;
/// Concurrency/parallelism utilities
pub mod threads;
#[cfg(feature = "timing")]
/// Timing utilities
pub mod timing;
