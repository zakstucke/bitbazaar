#![warn(clippy::disallowed_types)]
#![warn(missing_docs)]

//! bitbazaar - An assortment of publicly available cross-language utilities useful to my projects.

mod prelude;

/// Hello world function
pub fn hello() -> String {
    "Hello, World!".to_string()
}

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_hello() {
        let result = hello();
        assert_eq!(result, "Hello, World!".to_string());
    }
}
