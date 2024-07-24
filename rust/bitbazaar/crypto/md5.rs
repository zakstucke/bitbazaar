/// md5 hash function.
///
/// NOTE: this is an insecure hash function from modern standards.
/// Only use where you have to for backwards compatibility.
///
/// Arguments
/// - `input`: The input data to hash.
/// - `salt`: Optional salt `&str` to add to the input data.
pub fn md5(input: impl AsRef<[u8]>, salt: Option<&str>) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(input.as_ref());
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    format!("{:x}", hasher.finalize())
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
