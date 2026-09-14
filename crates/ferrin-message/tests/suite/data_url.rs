use ferrin_message::data_url::DEFAULT_MEDIA_TYPE;
use ferrin_message::data_url::is_data_url;
use ferrin_message::data_url::parse;
use pretty_assertions::assert_eq;

#[test]
fn parses_base64_payloads() {
    let parsed = parse("data:image/png;base64,iVBORw0KGgo=").unwrap();
    assert_eq!(parsed.media_type, "image/png");
    assert!(parsed.is_base64);
    assert_eq!(parsed.data.as_ref(), b"\x89PNG\r\n\x1a\n");

    let unpadded = parse("DATA:image/png;BASE64,iVBORw0KGgo").unwrap();
    assert_eq!(unpadded.data, parsed.data);

    let spaced = parse("data:text/plain;base64,aGVs\nbG8%3D").unwrap();
    assert_eq!(spaced.data.as_ref(), b"hello");
}

#[test]
fn parses_percent_encoded_text_payloads() {
    let parsed = parse("data:text/plain,hello%20world").unwrap();
    assert_eq!(parsed.media_type, "text/plain");
    assert!(!parsed.is_base64);
    assert_eq!(parsed.data.as_ref(), b"hello world");

    let with_comma = parse("data:,a,b").unwrap();
    assert_eq!(with_comma.media_type, DEFAULT_MEDIA_TYPE);
    assert_eq!(with_comma.data.as_ref(), b"a,b");
}

#[test]
fn keeps_parameters_other_than_base64() {
    let parsed = parse("data:text/plain;charset=utf-8;base64,aGk=").unwrap();
    assert_eq!(parsed.media_type, "text/plain;charset=utf-8");
    let only_charset = parse("data:;charset=utf-8,hi").unwrap();
    assert_eq!(only_charset.media_type, "text/plain;charset=utf-8");
    let empty_base64 = parse("data:;base64,").unwrap();
    assert_eq!(empty_base64.media_type, DEFAULT_MEDIA_TYPE);
    assert!(empty_base64.data.is_empty());
}

#[test]
fn rejects_malformed_input() {
    assert!(!is_data_url("http://example.com"));
    assert!(parse("http://example.com").is_err());
    assert!(parse("data:image/png;base64").is_err());
    let err = parse("data:image/png;base64,***").unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid data content: data url payload is not valid base64"
    );
    assert!(std::error::Error::source(&err).is_some());
}
