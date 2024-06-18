use serde::Deserialize;

use crate::log::record_exception;

/// Get and automatically decode a cookie into a deserializable type.
/// If the cookie isn't found, or if it fails to deserialize, returns None.
/// When it fails to deserialize, an error will be recorded.
pub fn get_cookie<T: for<'a> Deserialize<'a>>(name: &str) -> Option<T> {
    if let Some(value) = get_cookie_raw(name) {
        match serde_json::from_str(&value) {
            Ok(value) => Some(value),
            Err(e) => {
                record_exception("Failed to deserialize cookie value.", format!("{:?}", e));
                None
            }
        }
    } else {
        None
    }
}

/// Delete a cookie if it exists.
pub fn delete_cookie(name: &str) {
    // Easiest way to delete is to just set with instant expiry:
    set_cookie(
        name,
        &"",
        CookieOptions {
            expires: Some(std::time::Duration::from_secs(0)),
            ..Default::default()
        },
    );
}

/// Set a new cookie with the given name and serializable value.
/// If serialization fails, an error will be recorded.
pub fn set_cookie(name: &str, value: &impl serde::Serialize, options: CookieOptions<'_>) {
    match serde_json::to_string(value) {
        Ok(value) => set_cookie_raw(name, &value, options),
        Err(e) => {
            record_exception("Failed to serialize cookie value.", format!("{:?}", e));
        }
    };
}

/// Get the raw value of a cookie.
/// If the cookie isn't found, returns None.
pub fn get_cookie_raw(name: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use axum_extra::extract::cookie::CookieJar;
        if let Some(req) = leptos::use_context::<http::request::Parts>() {
            let cookies = CookieJar::from_headers(&req.headers);
            if let Some(cookie) = cookies.get(name) {
                return Some(cookie.value().to_string());
            }
        }
        None
    }

    #[cfg(target_arch = "wasm32")]
    {
        if let Some(Ok(value)) = wasm_cookies::get(name) {
            Some(value)
        } else {
            None
        }
    }
}

/// Set a new cookie with the given name and raw value.
pub fn set_cookie_raw(name: &str, value: &str, options: CookieOptions<'_>) {
    #[cfg(target_arch = "wasm32")]
    {
        wasm_cookies::set(name, value, &options.into())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        use axum_extra::extract::cookie::Cookie;

        let axum_response = leptos::expect_context::<leptos_axum::ResponseOptions>();
        let mut cookie = Cookie::build((name, value)).http_only(options.http_only);
        if let Some(path) = options.path {
            cookie = cookie.path(path);
        }
        if let Some(domain) = options.domain {
            cookie = cookie.domain(domain);
        }
        if let Some(expires) = options.expires {
            cookie = cookie.max_age(time::Duration::seconds(expires.as_secs() as i64));
        }
        if options.secure {
            cookie = cookie.secure(true);
        }
        cookie = match options.same_site {
            SameSite::Lax => cookie.same_site(axum_extra::extract::cookie::SameSite::Lax),
            SameSite::Strict => cookie.same_site(axum_extra::extract::cookie::SameSite::Strict),
            SameSite::None => cookie.same_site(axum_extra::extract::cookie::SameSite::None),
        };
        if let Ok(cookie) = http::HeaderValue::from_str(&cookie.to_string()) {
            axum_response.insert_header(http::header::SET_COOKIE, cookie);
        }
    }
}

/// Cookies options (see [https://developer.mozilla.org/en-US/docs/Web/API/Document/cookie](https://developer.mozilla.org/en-US/docs/Web/API/Document/cookie)).
///
/// You can create it by calling `CookieOptions::default()`.
#[derive(Default, Clone, Debug)]
pub struct CookieOptions<'a> {
    /// If `None`, defaults to the current path of the current document location.
    pub path: Option<&'a str>,

    /// If `None`, defaults to the host portion of the current document location.
    pub domain: Option<&'a str>,

    /// If `None`, the cookie will expire at the end of session.
    pub expires: Option<std::time::Duration>,

    /// If true, the cookie will only be transmitted over secure protocol as HTTPS.
    /// The default value is false.
    pub secure: bool,

    /// SameSite prevents the browser from sending the cookie along with cross-site requests
    /// (see [https://developer.mozilla.org/en-US/docs/Web/HTTP/Cookies#SameSite_attribute](https://developer.mozilla.org/en-US/docs/Web/HTTP/Cookies#SameSite_attribute)).
    pub same_site: SameSite,

    /// Only applicable to sever cookies. When true js/wasm cannot access the cookie.
    pub http_only: bool,
}

/// SameSite value for [CookieOptions](struct.CookieOptions.html).
///
/// SameSite prevents the browser from sending the cookie along with cross-site requests
/// (see [https://developer.mozilla.org/en-US/docs/Web/HTTP/Cookies#SameSite_attribute](https://developer.mozilla.org/en-US/docs/Web/HTTP/Cookies#SameSite_attribute)).
#[derive(Clone, Debug)]
pub enum SameSite {
    /// The `Lax` value value will send the cookie for all same-site requests and top-level navigation GET requests.
    /// This is sufficient for user tracking, but it will prevent many CSRF attacks.
    /// This is the default value when calling `SameSite::default()`.
    Lax,

    /// The `Strict` value will prevent the cookie from being sent by the browser to the
    /// target site in all cross-site browsing contexts, even when following a regular link.
    Strict,

    /// The `None` value explicitly states no restrictions will be applied.
    /// The cookie will be sent in all requests - both cross-site and same-site.
    None,
}
impl Default for SameSite {
    fn default() -> Self {
        Self::Lax
    }
}

#[cfg(target_arch = "wasm32")]
/// Conversion to the wasm_cookies which was originally created from:
impl<'a> From<CookieOptions<'a>> for wasm_cookies::CookieOptions<'a> {
    fn from(options: CookieOptions<'a>) -> Self {
        let mut inner = wasm_cookies::CookieOptions::default();
        inner.path = options.path;
        inner.domain = options.domain;
        if let Some(expires) = options.expires {
            inner = inner.expires_after(expires);
        }
        inner.secure = options.secure;
        inner.same_site = match options.same_site {
            SameSite::Lax => wasm_cookies::SameSite::Lax,
            SameSite::Strict => wasm_cookies::SameSite::Strict,
            SameSite::None => wasm_cookies::SameSite::None,
        };
        inner
    }
}
