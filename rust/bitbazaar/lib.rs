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
#[cfg(feature = "timing")]
/// Timing utilities
pub mod timing;
