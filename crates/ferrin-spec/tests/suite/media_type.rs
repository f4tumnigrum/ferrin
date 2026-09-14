use pretty_assertions::assert_eq;

use ferrin_spec::MediaType;

#[test]
fn detects_full_types() {
    assert!(MediaType::new("image/png").is_full());
    assert!(!MediaType::new("image/*").is_full());
    assert!(!MediaType::new("image").is_full());
    assert!(!MediaType::new("/png").is_full());
}

#[test]
fn extracts_top_level_and_subtype() {
    let media_type = MediaType::new("Image/PNG; q=1");
    assert_eq!(media_type.top_level(), "image");
    assert_eq!(media_type.subtype(), Some("PNG"));
    assert_eq!(MediaType::new("audio").subtype(), None);
    assert_eq!(MediaType::new("audio").top_level(), "audio");
}

#[test]
fn normalizes_wildcards_and_parameters() {
    assert_eq!(
        MediaType::new("image/*").normalize(),
        MediaType::new("image")
    );
    assert_eq!(
        MediaType::new("Text/Plain; charset=utf-8").normalize(),
        MediaType::new("text/plain")
    );
    assert_eq!(
        MediaType::new("image/").normalize(),
        MediaType::new("image")
    );
}

#[test]
fn matches_patterns() {
    let png = MediaType::new("image/png");
    assert!(png.matches(&MediaType::new("image/*")));
    assert!(png.matches(&MediaType::new("image")));
    assert!(png.matches(&MediaType::new("IMAGE/PNG")));
    assert!(!png.matches(&MediaType::new("image/jpeg")));
    assert!(!png.matches(&MediaType::new("audio")));
}

#[test]
fn serializes_transparently() {
    let media_type = MediaType::new("application/pdf");
    assert_eq!(
        serde_json::to_string(&media_type).unwrap(),
        r#""application/pdf""#
    );
    let parsed: MediaType = serde_json::from_str(r#""application/pdf""#).unwrap();
    assert_eq!(parsed, media_type);
}
