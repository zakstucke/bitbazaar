/// Url escape a string.
pub fn url_escape(s: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::js_sys::encode_uri_component(s)
            .as_string()
            .unwrap()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
    }
}

/// Decrypt a url escaped string.
pub fn url_unescape(s: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::js_sys::decode_uri_component(s).unwrap().into()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        percent_encoding::percent_decode_str(s)
            .decode_utf8()
            .unwrap()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_escape() {
        assert_eq!(url_escape("hello world"), "hello%20world");
        assert_eq!(url_escape("hello+world"), "hello%2Bworld");
        assert_eq!(url_escape("hello%world"), "hello%25world");
    }

    #[test]
    fn test_url_unescape() {
        assert_eq!(url_unescape("hello%20world"), "hello world");
        assert_eq!(url_unescape("hello%2Bworld"), "hello+world");
        assert_eq!(url_unescape("hello%25world"), "hello%world");
    }
}
