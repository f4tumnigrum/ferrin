use std::sync::Arc;

use bytes::Bytes;
use ferrin_provider_util::HttpRequest;
use ferrin_provider_util::HttpResponse;
use ferrin_provider_util::HttpTransport;
use ferrin_provider_util::RequestBody;
use ferrin_provider_util::TransportError;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use ferrin_testing::RecordingTransport;
use ferrin_testing::redact_secrets;
use ferrin_testing::transport::contains_secret;
use futures_util::StreamExt;
use futures_util::stream;
use http::StatusCode;
use pretty_assertions::assert_eq;
use url::Url;

struct ChunkedTransport;

impl HttpTransport for ChunkedTransport {
    fn execute(
        &self,
        _request: HttpRequest,
    ) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async {
            let chunks = stream::iter(vec![
                Ok(Bytes::from_static(b"ab")),
                Ok(Bytes::from_static(b"cd")),
            ]);
            Ok(HttpResponse::from_stream(
                StatusCode::OK,
                Headers::new()
                    .with("content-type", "text/plain")
                    .with("set-cookie", "secret=1"),
                Box::pin(chunks),
            ))
        })
    }
}

#[tokio::test]
async fn records_request_and_response_lazily() {
    let transport = RecordingTransport::new(Arc::new(ChunkedTransport));
    let request = HttpRequest::post(Url::parse("https://example.com/v1/x").unwrap())
        .with_headers(
            Headers::new()
                .with("authorization", "Bearer sk-secret")
                .with("x-request-id", "r1"),
        )
        .with_body(RequestBody::json(Bytes::from_static(b"{\"a\":1}")));
    let response = transport.execute(request).await.unwrap();

    let recorded = transport.last_request().unwrap();
    assert_eq!(recorded.method, http::Method::POST);
    assert_eq!(recorded.url.as_str(), "https://example.com/v1/x");
    assert!(!recorded.headers.contains("authorization"));
    assert_eq!(recorded.headers.get_str("x-request-id"), Some("r1"));
    assert_eq!(recorded.content_type.as_deref(), Some("application/json"));
    assert_eq!(recorded.body_json().unwrap(), serde_json::json!({"a": 1}));

    let snapshot = recorded.response();
    assert_eq!(snapshot.status, Some(StatusCode::OK));
    assert!(!snapshot.headers.contains("set-cookie"));
    assert!(snapshot.chunks.is_empty());
    assert!(!snapshot.complete);

    let mut body = response.body;
    assert_eq!(
        body.next().await.unwrap().unwrap(),
        Bytes::from_static(b"ab")
    );
    assert_eq!(recorded.response().chunks.len(), 1);
    assert_eq!(
        body.next().await.unwrap().unwrap(),
        Bytes::from_static(b"cd")
    );
    assert!(body.next().await.is_none());
    let snapshot = recorded.response();
    assert!(snapshot.complete);
    assert_eq!(snapshot.body_text(), "abcd");
    assert_eq!(transport.len(), 1);
}

#[tokio::test]
async fn allow_list_restricts_recorded_headers() {
    let transport = RecordingTransport::new(Arc::new(ChunkedTransport))
        .with_header_allow_list(["Content-Type", "authorization"]);
    let request = HttpRequest::get(Url::parse("https://example.com/").unwrap()).with_headers(
        Headers::new()
            .with("content-type", "text/plain")
            .with("authorization", "x")
            .with("x-other", "y"),
    );
    transport.execute(request).await.unwrap();
    let recorded = transport.last_request().unwrap();
    assert_eq!(recorded.headers.len(), 1);
    assert_eq!(recorded.headers.get_str("content-type"), Some("text/plain"));
    transport.clear();
    assert!(transport.is_empty());
}

#[test]
fn redacts_api_keys_and_bearer_tokens() {
    assert_eq!(
        redact_secrets("key sk-abcdefghijklmnop end"),
        "key [redacted] end"
    );
    assert_eq!(
        redact_secrets("Authorization: Bearer abc.def-ghi\nnext"),
        "Authorization: Bearer [redacted]\nnext"
    );
    assert_eq!(
        redact_secrets("{\"k\":\"Bearer tok\"}"),
        "{\"k\":\"Bearer [redacted]\"}"
    );
    assert_eq!(redact_secrets("sk-short"), "sk-short");
    assert!(contains_secret("sk-abcdefghijklmnop"));
    assert!(!contains_secret("plain text"));
}
