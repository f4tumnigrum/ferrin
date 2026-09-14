use bytes::Bytes;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

use ferrin_spec::FileData;

#[test]
fn bytes_round_trip_as_tagged_base64() {
    let data = FileData::bytes(Bytes::from_static(b"hello"));
    let value = serde_json::to_value(&data).unwrap();
    assert_eq!(value, json!({ "type": "data", "data": "aGVsbG8=" }));
    let parsed: FileData = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, data);
    assert_eq!(data.to_base64(), Some("aGVsbG8=".to_owned()));
}

#[test]
fn url_round_trip() {
    let url = Url::parse("https://example.com/a.png").unwrap();
    let data = FileData::url(url.clone());
    let value = serde_json::to_value(&data).unwrap();
    assert_eq!(
        value,
        json!({ "type": "url", "url": "https://example.com/a.png" })
    );
    let parsed: FileData = serde_json::from_value(value).unwrap();
    assert_eq!(parsed.as_url(), Some(&url));
}

#[test]
fn reference_and_text_round_trip() {
    let data = FileData::reference("openai", "file-abc");
    let value = serde_json::to_value(&data).unwrap();
    assert_eq!(
        value,
        json!({ "type": "reference", "reference": { "openai": "file-abc" } })
    );
    let parsed: FileData = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, data);
    assert_eq!(parsed.as_reference().unwrap()["openai"], "file-abc");

    let text = FileData::text("plain document");
    let value = serde_json::to_value(&text).unwrap();
    assert_eq!(value, json!({ "type": "text", "text": "plain document" }));
    assert_eq!(text.as_text(), Some("plain document"));
    assert_eq!(text.as_bytes(), None);
}

#[test]
fn accepts_unpadded_base64() {
    let data = FileData::from_base64("aGVsbG8").unwrap();
    assert_eq!(data.as_bytes().unwrap().as_ref(), b"hello");
    let parsed: FileData =
        serde_json::from_value(json!({ "type": "data", "data": "aGVsbG8" })).unwrap();
    assert_eq!(parsed, data);
}

#[test]
fn rejects_invalid_base64_and_unknown_tags() {
    let err = serde_json::from_value::<FileData>(json!({ "type": "data", "data": "not base64!" }))
        .unwrap_err();
    assert!(err.to_string().contains("Invalid"), "{err}");
    let err = serde_json::from_value::<FileData>(json!({ "type": "blob" })).unwrap_err();
    assert!(err.to_string().contains("unknown variant"), "{err}");
}
