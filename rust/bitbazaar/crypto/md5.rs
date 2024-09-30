/// md5 hash function.
///
/// NOTE: this is an insecure hash function from modern standards.
/// Only use where you have to for backwards compatibility.
///
/// Arguments
/// - `input`: The input data to hash.
/// - `salt`: Optional salt `&str` to add to the input data.
pub fn md5(input: impl AsRef<[u8]>, salt: Option<&str>) -> String {
    let out = md5_raw(std::iter::once(input.as_ref()).chain(salt.map(|s| s.as_bytes())));
    format!("{:x}", out)
}

/// md5 hash function, lower level than [`md5`] to allow multiple inputs and byte output.
///
/// NOTE: this is an insecure hash function from modern standards.
/// Only use where you have to for backwards compatibility.
///
/// Arguments
/// - `inputs`: The input data.
pub fn md5_raw<B: AsRef<[u8]>>(
    inputs: impl IntoIterator<Item = B>,
) -> impl std::fmt::LowerHex + std::convert::AsRef<[u8]> + 'static {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    for input in inputs {
        hasher.update(input.as_ref());
    }
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5() {
        assert_eq!(md5("hello", None), "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(
            md5("hello", Some("world")),
            "fc5e038d38a57032085441e7fe7010b0"
        );
    }
}
