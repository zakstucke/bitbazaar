#![allow(clippy::module_inception)]
#![allow(clippy::type_complexity)]
#![warn(clippy::disallowed_types)]
#![warn(missing_docs)]

//! bitbazaar - An assortment of publicly available cross-language utilities useful to my projects.

mod prelude;

#[cfg(feature = "cli")]
/// Command line interface utilities.
pub mod cli;

#[cfg(feature = "chrono")]
/// Chrono utilities
pub mod chrono;
#[cfg(feature = "cookies")]
/// Setting/getting cookies in frontend or ssr.
pub mod cookies;
/// Error handling utilities.
pub mod errors;
#[cfg(feature = "hash")]
/// Hashing utilities.
pub mod hash;
/// Logging utilities
pub mod log;
/// Completely miscellaneous utilities
pub mod misc;
#[cfg(feature = "redis")]
/// Redis utilities
pub mod redis;
/// Concurrency/parallelism utilities
pub mod threads;
#[cfg(feature = "timing")]
/// Timing utilities
pub mod timing;
