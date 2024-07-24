/// SHA256 hash function.
///
/// Arguments
/// - `input`: The input data to hash.
/// - `salt`: Optional salt `&str` to add to the input data.
pub fn sha256(input: impl AsRef<[u8]>, salt: Option<&str>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_ref());
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    format!("{:x}", hasher.finalize())
}
