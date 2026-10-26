//! Header rules.
//!
//! The host owns the authority and the framing of every request it makes for
//! a Plugin, so a header list from a Plugin, an `attach` rule, a live
//! channel or a Stream Source must not name them (`plugin-api/wit/host.wit`,
//! `http.request`). Responses are bounded: at most [`MAX_RESPONSE_HEADERS`]
//! headers and [`MAX_RESPONSE_HEADER_BYTES`] bytes.

use http::header::{HeaderMap, HeaderName, HeaderValue};

use crate::Error;

/// A header as a name and a value.
pub type Header = (String, String);

/// Most headers a response may have.
pub const MAX_RESPONSE_HEADERS: usize = 200;

/// Most bytes a response's headers may take, counting each header's name
/// and value.
pub const MAX_RESPONSE_HEADER_BYTES: usize = 64 * 1024;

/// Whether `name` is one the host sets itself: `Host`, `Content-Length`,
/// `Transfer-Encoding`, `Connection`, `Upgrade`, `Keep-Alive`, `TE`,
/// `Trailer`, any `Proxy-*` header, or an HTTP/2 pseudo-header.
pub fn is_forbidden(name: &str) -> bool {
    let name = name.trim();
    if name.starts_with(':') {
        return true;
    }
    const FORBIDDEN: [&str; 8] = [
        "host",
        "content-length",
        "transfer-encoding",
        "connection",
        "upgrade",
        "keep-alive",
        "te",
        "trailer",
    ];
    FORBIDDEN
        .iter()
        .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
        || name
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("proxy-"))
}

/// Checks a header list someone other than the host wrote and converts it.
/// A forbidden or malformed name, or a value with control characters, fails
/// the whole request before anything is sent.
pub fn checked(headers: &[Header]) -> Result<Vec<(HeaderName, HeaderValue)>, Error> {
    headers
        .iter()
        .map(|(name, value)| {
            if is_forbidden(name) {
                return Err(Error::NotAllowed(format!(
                    "header {name} is set by the host"
                )));
            }
            let parsed_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Error::NotAllowed(format!("header name {name:?} is malformed")))?;
            let parsed_value = HeaderValue::from_str(value)
                .map_err(|_| Error::NotAllowed(format!("value of header {name} is malformed")))?;
            Ok((parsed_name, parsed_value))
        })
        .collect()
}

/// Converts response headers, checking the bounds. Values that are not UTF-8
/// are read as ISO-8859-1, HTTP's historical header charset.
pub(crate) fn from_response(map: &HeaderMap) -> Result<Vec<Header>, Error> {
    if map.len() > MAX_RESPONSE_HEADERS {
        return Err(Error::TooLarge(format!(
            "response has more than {MAX_RESPONSE_HEADERS} headers"
        )));
    }
    let mut bytes = 0usize;
    let mut out = Vec::with_capacity(map.len());
    for (name, value) in map {
        bytes += name.as_str().len() + value.len();
        if bytes > MAX_RESPONSE_HEADER_BYTES {
            return Err(Error::TooLarge(format!(
                "response headers exceed {MAX_RESPONSE_HEADER_BYTES} bytes"
            )));
        }
        out.push((name.as_str().to_owned(), value_text(value.as_bytes())));
    }
    Ok(out)
}

fn value_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&byte| char::from(byte)).collect(),
    }
}

/// The first value of `name`.
pub fn get<'a>(headers: &'a [Header], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Every value of `name`, joined as a list header would be.
pub fn joined(headers: &[Header], name: &str) -> Option<String> {
    let values: Vec<&str> = headers
        .iter()
        .filter(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

/// Whether the list names `name`.
pub fn has(headers: &[Header], name: &str) -> bool {
    headers
        .iter()
        .any(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_and_authority_headers_are_forbidden() {
        for name in [
            "Host",
            "content-length",
            "Transfer-Encoding",
            "CONNECTION",
            "Upgrade",
            "Keep-Alive",
            "TE",
            "Trailer",
            "Proxy-Authorization",
            "proxy-connection",
            ":authority",
            ":path",
        ] {
            assert!(is_forbidden(name), "{name}");
            let error = checked(&[(name.to_owned(), "x".to_owned())]).unwrap_err();
            assert!(matches!(error, Error::NotAllowed(_)), "{name}");
        }
        for name in [
            "Authorization",
            "Cookie",
            "Accept",
            "Range",
            "Proxy",
            "Hosts",
            "Tea",
        ] {
            assert!(!is_forbidden(name), "{name}");
        }
    }

    #[test]
    fn header_injection_is_refused() {
        assert!(checked(&[("X-A".into(), "a\r\nHost: evil".into())]).is_err());
        assert!(checked(&[("X-A\r\nHost".into(), "a".into())]).is_err());
        assert!(checked(&[("".into(), "a".into())]).is_err());
        assert!(checked(&[("X-A".into(), "fine".into())]).is_ok());
    }

    #[test]
    fn response_header_bounds_hold() {
        let mut map = HeaderMap::new();
        for index in 0..MAX_RESPONSE_HEADERS {
            map.append(
                HeaderName::from_bytes(format!("x-{index}").as_bytes()).unwrap(),
                HeaderValue::from_static("v"),
            );
        }
        assert_eq!(from_response(&map).unwrap().len(), MAX_RESPONSE_HEADERS);
        map.append("x-one-more", HeaderValue::from_static("v"));
        assert!(matches!(from_response(&map), Err(Error::TooLarge(_))));

        let mut big = HeaderMap::new();
        let value = "v".repeat(MAX_RESPONSE_HEADER_BYTES);
        big.insert("x-big", HeaderValue::from_str(&value).unwrap());
        assert!(matches!(from_response(&big), Err(Error::TooLarge(_))));
    }

    #[test]
    fn latin1_values_survive() {
        let mut map = HeaderMap::new();
        map.insert("x-name", HeaderValue::from_bytes(b"caf\xe9").unwrap());
        assert_eq!(from_response(&map).unwrap()[0].1, "café");
    }
}
