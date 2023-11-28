#![warn(clippy::disallowed_types)]

pub mod utils;

pub fn hello() -> String {
    "Hello, World!".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello() {
        let result = hello();
        assert_eq!(result, "Hello, World!".to_string());
    }
}
