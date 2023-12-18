const FNV1A_PRIME: u64 = 1099511628211;
const FNV1A_OFFSET_BASIS: u64 = 14695981039346656037;

pub fn fnv1a(input: &[u8]) -> u64 {
    let mut hash = FNV1A_OFFSET_BASIS;

    for byte in input {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV1A_PRIME);
    }

    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fnv1a() {
        // Empty should be basis:
        assert_eq!(fnv1a(b""), FNV1A_OFFSET_BASIS);

        let hello = fnv1a(b"Hello");
        assert_ne!(hello, FNV1A_OFFSET_BASIS);
        assert_eq!(fnv1a(b"Hello"), hello);

        let world = fnv1a(b"World");
        assert_ne!(world, FNV1A_OFFSET_BASIS);
        assert_ne!(world, hello);
        assert_eq!(fnv1a(b"World"), world);
    }
}
