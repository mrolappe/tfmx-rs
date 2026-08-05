//! Parses a request URL's query string into a `key -> value` map.
//! `tiny_http` hands back the raw request-line URL undecoded, so this also
//! percent-decodes each value (byte-wise, not `str`-slicing, so a stray `%`
//! next to a multi-byte UTF-8 character can't land mid-codepoint and panic).

use std::collections::HashMap;

pub fn parse(url: &str) -> HashMap<String, String> {
    let Some((_, query)) = url.split_once('?') else {
        return HashMap::new();
    };
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => match (
                bytes.get(i + 1).copied().and_then(hex_val),
                bytes.get(i + 2).copied().and_then(hex_val),
            ) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_key_value_pairs() {
        let q = parse("/song-view?song=3");
        assert_eq!(q.get("song").map(String::as_str), Some("3"));
    }

    #[test]
    fn a_url_with_no_query_string_yields_an_empty_map() {
        assert!(parse("/files").is_empty());
    }

    #[test]
    fn percent_and_plus_encoded_values_are_decoded() {
        let q = parse("/files?dir=testdata%2Fturrican+intro");
        assert_eq!(
            q.get("dir").map(String::as_str),
            Some("testdata/turrican intro")
        );
    }

    #[test]
    fn a_trailing_percent_sign_is_kept_literally_instead_of_panicking() {
        let q = parse("/files?dir=100%");
        assert_eq!(q.get("dir").map(String::as_str), Some("100%"));
    }
}
