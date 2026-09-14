use regex::Regex;
use url::Url;

use ferrin_spec::MediaType;
use ferrin_spec::SupportedUrls;

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

#[test]
fn matches_by_media_type_prefix_and_url_pattern() {
    let supported = SupportedUrls::none()
        .with("image/*", [Regex::new(r"^https://.*$").unwrap()])
        .with(
            "application/pdf",
            [Regex::new(r"^https://docs\.example\.com/").unwrap()],
        );

    assert!(supported.supports(&MediaType::new("image/png"), &url("https://x.test/a.png")));
    assert!(supported.supports(&MediaType::new("IMAGE/PNG"), &url("HTTPS://x.test/a.png")));
    assert!(!supported.supports(&MediaType::new("image/png"), &url("http://x.test/a.png")));
    assert!(supported.supports(&MediaType::new("image"), &url("https://x.test/a.png")));
    assert!(!supported.supports(
        &MediaType::new("application"),
        &url("https://docs.example.com/f")
    ));
    assert!(supported.supports(
        &MediaType::new("application/pdf"),
        &url("https://docs.example.com/f")
    ));
    assert!(!supported.supports(&MediaType::new("audio/mp3"), &url("https://x.test/a.mp3")));
}

#[test]
fn wildcard_key_matches_every_media_type() {
    let supported = SupportedUrls::all();
    assert!(supported.supports(&MediaType::new("audio/mp3"), &url("https://x.test/a.mp3")));
    assert!(supported.supports(&MediaType::new("video"), &url("ftp://x.test/a")));
    assert!(SupportedUrls::none().is_empty());
    assert!(!supported.is_empty());
}

#[test]
fn insert_extends_existing_entry() {
    let mut supported = SupportedUrls::none();
    supported.insert("image/*", [Regex::new("^https://a/").unwrap()]);
    supported.insert("IMAGE/*", [Regex::new("^https://b/").unwrap()]);
    assert_eq!(supported.iter().count(), 1);
    assert!(supported.supports(&MediaType::new("image/png"), &url("https://b/x")));
}
