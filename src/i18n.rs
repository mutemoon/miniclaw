use axum::http::HeaderMap;

pub fn get_locale_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("accept-language")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.split(',').next())
        .and_then(|h| h.split(';').next())
        .map(|h| h.trim().to_string())
        .unwrap_or_else(|| rust_i18n::locale().to_string())
}

#[macro_export]
macro_rules! with_locale {
    ($headers:expr, $block:block) => {{
        let locale = crate::i18n::get_locale_from_headers($headers);
        rust_i18n::set_locale(&locale);
        let result = $block;
        result
    }};
}
