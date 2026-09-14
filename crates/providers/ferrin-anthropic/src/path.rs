//! URL path helpers.

/// Percent-encodes one path segment like `encodeURIComponent`; the segments
/// `.` and `..` are double-encoded so that they cannot change the path.
#[must_use]
pub(crate) fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => encoded.push(char::from(byte)),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    match encoded.as_str() {
        "." => "%252E".to_owned(),
        ".." => "%252E%252E".to_owned(),
        _ => encoded,
    }
}
