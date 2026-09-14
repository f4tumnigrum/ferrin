use bytes::Bytes;
use ferrin_message::FileData;
use ferrin_message::FileSource;
use ferrin_message::FileSourceError;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

#[test]
fn serializes_with_type_tags() {
    let cases = [
        (
            FileSource::bytes(Bytes::from_static(b"hi")),
            json!({ "type": "data", "data": "aGk=" }),
        ),
        (
            FileSource::base64("aGk"),
            json!({ "type": "base64", "data": "aGk" }),
        ),
        (
            FileSource::parse_url("https://example.com/a.pdf").unwrap(),
            json!({ "type": "url", "url": "https://example.com/a.pdf" }),
        ),
        (
            FileSource::reference("openai", "file-1"),
            json!({ "type": "reference", "reference": { "openai": "file-1" } }),
        ),
        (
            FileSource::text("doc"),
            json!({ "type": "text", "text": "doc" }),
        ),
        (
            FileSource::path("/tmp/a.pdf"),
            json!({ "type": "path", "path": "/tmp/a.pdf" }),
        ),
    ];
    for (source, expected) in cases {
        let value = serde_json::to_value(&source).unwrap();
        assert_eq!(value, expected);
        let parsed: FileSource = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, source);
    }
}

#[test]
fn converts_to_file_data_without_io() {
    assert_eq!(
        FileData::try_from(FileSource::base64("aGk=")).unwrap(),
        FileData::bytes(Bytes::from_static(b"hi"))
    );
    let url = Url::parse("data:text/plain,hi").unwrap();
    assert_eq!(
        FileData::try_from(FileSource::url(url.clone())).unwrap(),
        FileData::url(url)
    );
    assert_eq!(
        FileData::try_from(FileSource::text("doc")).unwrap(),
        FileData::text("doc")
    );
    assert!(matches!(
        FileData::try_from(FileSource::path("/tmp/x")),
        Err(FileSourceError::UnreadPath { .. })
    ));
    assert!(matches!(
        FileData::try_from(FileSource::base64("***")),
        Err(FileSourceError::InvalidDataContent(_))
    ));
    assert_eq!(
        FileSource::from(FileData::reference("p", "id")),
        FileSource::reference("p", "id")
    );
}

#[test]
fn inspects_inline_content_and_data_urls() {
    let source = FileSource::parse_url("data:image/png;base64,aGk=").unwrap();
    assert!(source.is_data_url());
    let parsed = source.data_url().unwrap().unwrap();
    assert_eq!(parsed.media_type, "image/png");
    assert_eq!(parsed.data.as_ref(), b"hi");
    assert!(FileSource::text("x").data_url().is_none());

    assert_eq!(
        FileSource::base64("aG k=").decoded_bytes().unwrap(),
        Some(Bytes::from_static(b"hi"))
    );
    assert_eq!(FileSource::text("x").decoded_bytes().unwrap(), None);
    assert!(FileSource::base64("!").decoded_bytes().is_err());
}
