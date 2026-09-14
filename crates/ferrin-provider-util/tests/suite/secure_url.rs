use std::net::IpAddr;

use ferrin_provider_util::UrlPolicy;
use ferrin_provider_util::http::ReqwestTransport;
use ferrin_provider_util::secure_url::DownloadErrorKind;
use ferrin_provider_util::secure_url::UrlValidationError;
use ferrin_provider_util::secure_url::fetch;
use ferrin_provider_util::secure_url::fetch_with_headers;
use ferrin_provider_util::secure_url::is_private_hostname;
use ferrin_provider_util::secure_url::is_private_ip;
use ferrin_provider_util::secure_url::validate_url;
use ferrin_spec::Headers;
use ferrin_spec::MediaType;
use pretty_assertions::assert_eq;
use tokio_util::sync::CancellationToken;
use url::Url;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::header_exists;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

fn local_policy() -> UrlPolicy {
    UrlPolicy::new().allow_http().allow_private_networks()
}

#[test]
fn private_ranges_are_detected() {
    for blocked in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "224.0.0.1",
        "240.0.0.1",
        "192.0.2.1",
        "198.18.0.1",
        "::1",
        "::",
        "fc00::1",
        "fd12::1",
        "fe80::1",
        "ff02::1",
        "2001:db8::1",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "64:ff9b::7f00:1",
        "2002:7f00:1::",
    ] {
        assert!(is_private_ip(ip(blocked)), "{blocked}");
    }
    for allowed in [
        "8.8.8.8",
        "1.1.1.1",
        "172.32.0.1",
        "2606:4700::1111",
        "::ffff:8.8.8.8",
    ] {
        assert!(!is_private_ip(ip(allowed)), "{allowed}");
    }
    assert!(is_private_hostname("localhost"));
    assert!(is_private_hostname("Foo.LOCALHOST."));
    assert!(is_private_hostname("printer.local"));
    assert!(is_private_hostname("[::1]"));
    assert!(!is_private_hostname("example.com"));
}

#[tokio::test]
async fn validation_rules() {
    let policy = UrlPolicy::new();
    assert_eq!(
        validate_url(&url("http://example.com/a"), &policy)
            .await
            .unwrap_err(),
        UrlValidationError::SchemeNotAllowed {
            scheme: "http".to_owned()
        }
    );
    assert_eq!(
        validate_url(&url("https://user:pw@example.com/a"), &policy)
            .await
            .unwrap_err(),
        UrlValidationError::EmbeddedCredentials
    );
    assert_eq!(
        validate_url(&url("https://127.0.0.1/a"), &policy)
            .await
            .unwrap_err(),
        UrlValidationError::PrivateAddress {
            host: "127.0.0.1".to_owned(),
            address: ip("127.0.0.1"),
        }
    );
    assert_eq!(
        validate_url(&url("https://localhost/a"), &policy)
            .await
            .unwrap_err(),
        UrlValidationError::PrivateHost {
            host: "localhost".to_owned()
        }
    );
    let trusted = UrlPolicy::new().trust_origin(&url("https://localhost"));
    let validated = validate_url(&url("https://localhost/a"), &trusted)
        .await
        .unwrap();
    assert!(validated.addresses.is_empty());
    let validated = validate_url(&url("https://[2606:4700::1111]:8443/a"), &policy)
        .await
        .unwrap();
    assert_eq!(validated.addresses.len(), 1);
    assert_eq!(validated.addresses[0].port(), 8443);
}

#[tokio::test]
async fn fetch_downloads_with_media_type_and_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/cat.png"))
        .and(header_exists("user-agent"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "image/png; charset=binary")
                .set_body_bytes(vec![1u8; 32]),
        )
        .mount(&server)
        .await;
    let transport = ReqwestTransport::new().unwrap();
    let target = url(&format!("{}/cat.png", server.uri()));
    let downloaded = fetch(
        &transport,
        target.clone(),
        &local_policy(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(downloaded.url, target);
    assert_eq!(downloaded.data.len(), 32);
    assert_eq!(downloaded.media_type, Some(MediaType::new("image/png")));

    let limited = local_policy().max_body_bytes(8);
    let error = fetch(&transport, target, &limited, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error.kind(), DownloadErrorKind::Transport(_)));
    assert!(error.to_string().contains("exceeds the limit"));
}

#[tokio::test]
async fn fetch_rejects_private_urls_and_bad_status() {
    let transport = ReqwestTransport::new().unwrap();
    let error = fetch(
        &transport,
        url("https://127.0.0.1/secret"),
        &UrlPolicy::new(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error.kind(), DownloadErrorKind::Validation(_)));

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let error = fetch(
        &transport,
        url(&format!("{}/missing", server.uri())),
        &local_policy(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error.kind(), DownloadErrorKind::Status { status, .. } if status.as_u16() == 404)
    );
}

#[tokio::test]
async fn redirects_are_validated_and_strip_headers_cross_origin() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    let final_url = format!("{}/final", second.uri());
    Mock::given(method("GET"))
        .and(path("/start"))
        .and(header("authorization", "Bearer k"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", final_url.as_str()))
        .mount(&first)
        .await;
    Mock::given(method("GET"))
        .and(path("/final"))
        .respond_with(ResponseTemplate::new(200).set_body_string("done"))
        .mount(&second)
        .await;
    let transport = ReqwestTransport::new().unwrap();
    let start = url(&format!("{}/start", first.uri()));
    let policy = local_policy().credential_origin(&start);
    let downloaded = fetch_with_headers(
        &transport,
        start.clone(),
        Headers::new().with("authorization", "Bearer k"),
        &policy,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(downloaded.url.as_str(), final_url);
    assert_eq!(&downloaded.data[..], b"done");
    let received = second.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    assert!(!received[0].headers.contains_key("authorization"));
    assert!(received[0].headers.contains_key("user-agent"));

    let no_redirects = local_policy().credential_origin(&start).max_redirects(0);
    let error = fetch_with_headers(
        &transport,
        start,
        Headers::new().with("authorization", "Bearer k"),
        &no_redirects,
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error.kind(),
        DownloadErrorKind::TooManyRedirects { limit: 0 }
    ));
}

#[tokio::test]
async fn authorization_is_dropped_for_uncredentialed_origins() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    let transport = ReqwestTransport::new().unwrap();
    fetch_with_headers(
        &transport,
        url(&format!("{}/a", server.uri())),
        Headers::new()
            .with("authorization", "Bearer k")
            .with("cookie", "x=1"),
        &local_policy(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let received = server.received_requests().await.unwrap();
    assert!(!received[0].headers.contains_key("authorization"));
    assert!(!received[0].headers.contains_key("cookie"));
}
