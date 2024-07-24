/// Generate random bytes using the system's random number generator.
/// Insecure, but very fast, useful for testing random binary data.
pub fn random_bytes_insecure_speedy(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for _ in 0..size {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        data.push((seed >> 32) as u8);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_bytes_insecure_speedy() {
        let data = random_bytes_insecure_speedy(10);
        assert_eq!(data.len(), 10);
        // Make sure not all the same number:
        assert!(data.iter().any(|&x| x != data[0]));
    }
}
