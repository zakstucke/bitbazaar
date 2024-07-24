#[cfg(feature = "aes256")]
/// aes256 encryption utilities.
pub mod aes256;

/// FNV1a hashing utilities. No dependencies hence not feature gated.
pub mod fnv1a;

/// MD5 hashing utilities.
#[cfg(feature = "md5")]
pub mod md5;

#[cfg(feature = "password")]
/// Password management utilities.
pub mod password;

#[cfg(feature = "sha")]
/// SHA related utilities.
pub mod sha;
