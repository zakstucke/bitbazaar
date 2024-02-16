use rustc_version::{version_meta, Channel};

// This build script sets the `cfg` flag `CHANNEL_STABLE`, `CHANNEL_BETA`, `CHANNEL_NIGHTLY` or `CHANNEL_DEV` depending on the release channel of the compiler being used.
// https://stackoverflow.com/questions/61417452/how-to-get-a-feature-requirement-tag-in-the-documentation-generated-by-cargo-do
fn main() {
    // Set cfg flags depending on release channel
    let channel = match version_meta().unwrap().channel {
        Channel::Stable => "CHANNEL_STABLE",
        Channel::Beta => "CHANNEL_BETA",
        Channel::Nightly => "CHANNEL_NIGHTLY",
        Channel::Dev => "CHANNEL_DEV",
    };
    println!("cargo:rustc-cfg={}", channel)
}
