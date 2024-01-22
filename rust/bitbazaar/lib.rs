#![warn(clippy::disallowed_types)]
#![warn(missing_docs)]

//! BitBazaar - A crate containing miscellaneous public utilities.

#[cfg(feature = "cli")]
/// Command line interface utilities.
pub mod cli;

/// Error handling utilities.
pub mod errors;
/// Hashing utilities.
pub mod hash;
/// Logging utilities
pub mod logging;
/// Completely miscellaneous utilities
pub mod misc;
#[cfg(feature = "redis")]
/// Redis utilities
pub mod redis;
/// Timing utilities
pub mod timing;
