use std::time::Duration;

use ferrin_provider_util::ResponseHandlers;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::http::MultipartForm;
use ferrin_provider_util::http::ParseResult;
use ferrin_provider_util::http::RequestBody;
use ferrin_provider_util::http::ReqwestTransport;
use ferrin_provider_util::http::binary_response_handler;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_error_response_handler;
use ferrin_provider_util::http::json_lines_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_form;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::http::status_code_error_response_handler;
use ferrin_spec::Headers;
use ferrin_spec::error::ProviderError;
use futures_util::StreamExt;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_json;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::helpers::api_error;

#[derive(Debug, Deserialize, PartialEq)]
struct Reply {
    id: String,
    value: u32,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    message: String,
    code: Option<String>,
}

fn handlers() -> ResponseHandlers<Reply> {
    ResponseHandlers::new(
        json_response_handler::<Reply>(),
        json_error_response_handler::<ApiError>(|error| error.error.message.clone()),
    )
}

fn transport() -> ReqwestTransport {
    ReqwestTransport::new().unwrap()
}

fn url(server: &MockServer, path: &str) -> Url {
    Url::parse(&format!("{}{path}", server.uri())).unwrap()
}

#[tokio::test]
async fn post_json_parses_success_and_keeps_raw() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/things"))
        .and(header("content-type", "application/json"))
        .and(header("authorization", "Bearer k"))
        .and(body_json(json!({ "name": "x" })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-request-id", "req_1")
                .set_body_json(json!({ "id": "t_1", "value": 3, "extra": true })),
        )
        .mount(&server)
        .await;

    let response = post_json(
        &transport(),
        url(&server, "/v1/things"),
        Headers::new().with("authorization", "Bearer k"),
        &json!({ "name": "x" }),
        &handlers(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        response.value,
        Reply {
            id: "t_1".to_owned(),
            value: 3,
        }
    );
    assert_eq!(
        response.response_headers.get_str("x-request-id"),
        Some("req_1")
    );
    assert_eq!(
        response.raw,
        Some(json!({ "id": "t_1", "value": 3, "extra": true }))
    );
}

#[tokio::test]
async fn error_handler_builds_api_call_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": { "message": "slow down", "code": "rate_limit" }
        })))
        .mount(&server)
        .await;

    let error = post_json(
        &transport(),
        url(&server, "/v1/things"),
        Headers::new(),
        &json!({ "name": "x" }),
        &handlers(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let api = api_error(&error);
    assert_eq!(api.message, "slow down");
    assert_eq!(api.status_code, Some(StatusCode::TOO_MANY_REQUESTS));
    assert!(api.is_retryable);
    assert_eq!(api.request_body, Some(json!({ "name": "x" })));
    assert_eq!(
        api.data,
        Some(json!({ "error": { "message": "slow down", "code": "rate_limit" } }))
    );
    assert!(api.response_body.as_deref().unwrap().contains("rate_limit"));
}

#[tokio::test]
async fn unparsable_error_body_falls_back_to_status_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("<html>down</html>"))
        .mount(&server)
        .await;
    let error = post_json(
        &transport(),
        url(&server, "/x"),
        Headers::new(),
        &json!({}),
        &handlers(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let api = api_error(&error);
    assert_eq!(api.message, "Service Unavailable");
    assert!(api.is_retryable);
    assert_eq!(api.data, None);
}

#[tokio::test]
async fn retryable_override_is_applied() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "overloaded", "code": "overloaded_error" }
        })))
        .mount(&server)
        .await;
    let handlers = ResponseHandlers::new(
        json_response_handler::<Reply>(),
        json_error_response_handler::<ApiError>(|error| error.error.message.clone())
            .with_is_retryable(|_head, error| {
                error.is_some_and(|error| error.error.code.as_deref() == Some("overloaded_error"))
            }),
    );
    let error = post_json(
        &transport(),
        url(&server, "/x"),
        Headers::new(),
        &json!({}),
        &handlers,
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(api_error(&error).is_retryable);
}

#[tokio::test]
async fn invalid_success_json_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{\"id\": 1}"))
        .mount(&server)
        .await;
    let error = post_json(
        &transport(),
        url(&server, "/x"),
        Headers::new(),
        &json!({}),
        &handlers(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let api = api_error(&error);
    assert_eq!(api.message, "invalid JSON response");
    assert_eq!(api.status_code, Some(StatusCode::OK));
    assert_eq!(api.response_body.as_deref(), Some("{\"id\": 1}"));
    assert!(!api.is_retryable);
}

#[tokio::test]
async fn connection_failure_is_retryable_api_call_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let target = Url::parse(&format!("http://127.0.0.1:{port}/x")).unwrap();
    let error = post_json(
        &transport(),
        target,
        Headers::new(),
        &json!({}),
        &handlers(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let api = api_error(&error);
    assert!(api.message.starts_with("cannot connect to API"));
    assert!(api.is_retryable);
    assert_eq!(api.status_code, None);
}

#[tokio::test]
async fn cancellation_surfaces_as_cancelled() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;
    let token = CancellationToken::new();
    let cancel = token.clone();
    let transport = transport();
    let body = json!({});
    let handlers = handlers();
    let request = post_json(
        &transport,
        url(&server, "/x"),
        Headers::new(),
        &body,
        &handlers,
        token,
    );
    let (result, ()) = tokio::join!(request, async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
    });
    assert!(matches!(result.unwrap_err(), ProviderError::Cancelled));
}

#[tokio::test]
async fn event_source_handler_streams_chunks() {
    let server = MockServer::start().await;
    let body = "data: {\"id\":\"a\",\"value\":1}\n\ndata: not json\n\ndata: {\"id\":\"b\",\"value\":2}\n\ndata: [DONE]\n\n";
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;
    let handlers = ResponseHandlers::new(
        event_source_response_handler::<Reply>(),
        status_code_error_response_handler(),
    );
    let response = get(
        &transport(),
        url(&server, "/stream"),
        Headers::new(),
        &handlers,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let items: Vec<ParseResult<Reply>> = response.value.collect().await;
    assert_eq!(items.len(), 3);
    assert!(matches!(&items[0], ParseResult::Ok { value, .. } if value.id == "a"));
    assert!(matches!(&items[1], ParseResult::Err { raw: Some(raw), .. } if raw == "not json"));
    assert!(
        matches!(&items[2], ParseResult::Ok { value, raw } if value.id == "b" && raw["value"] == 2)
    );
}

#[tokio::test]
async fn json_lines_handler_streams_lines() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("{\"id\":\"a\",\"value\":1}\r\n\n{\"id\":\"b\",\"value\":2}"),
        )
        .mount(&server)
        .await;
    let handlers = ResponseHandlers::new(
        json_lines_response_handler::<Reply>(),
        status_code_error_response_handler(),
    );
    let response = get(
        &transport(),
        url(&server, "/lines"),
        Headers::new(),
        &handlers,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let ids: Vec<String> = response
        .value
        .map(|item| item.into_result().unwrap().id)
        .collect()
        .await;
    assert_eq!(ids, vec!["a".to_owned(), "b".to_owned()]);
}

#[tokio::test]
async fn binary_handler_enforces_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![7u8; 64]))
        .mount(&server)
        .await;
    let ok = ResponseHandlers::new(
        binary_response_handler(),
        status_code_error_response_handler(),
    );
    let response = get(
        &transport(),
        url(&server, "/bin"),
        Headers::new(),
        &ok,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(response.value.len(), 64);

    let limited = ResponseHandlers::new(
        binary_response_handler().with_max_bytes(16),
        status_code_error_response_handler(),
    );
    let error = get(
        &transport(),
        url(&server, "/bin"),
        Headers::new(),
        &limited,
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let api = api_error(&error);
    assert!(api.message.contains("exceeds the limit"));
    assert!(!api.is_retryable);
}

#[tokio::test]
async fn multipart_form_is_encoded() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header(
            "content-type",
            "multipart/form-data; boundary=fixed",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "f", "value": 1 })))
        .mount(&server)
        .await;
    let form = MultipartForm::with_boundary("fixed")
        .field("model", "whisper-1")
        .file(
            "file",
            Some("a.mp3".to_owned()),
            Some("audio/mpeg".to_owned()),
            vec![1, 2, 3].into(),
        );
    let encoded = String::from_utf8_lossy(&form.encode()).into_owned();
    assert!(encoded.starts_with(
        "--fixed\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-1\r\n--fixed\r\n"
    ));
    assert!(encoded.contains(
        "filename=\"a.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\n\u{1}\u{2}\u{3}\r\n--fixed--\r\n"
    ));
    assert_eq!(
        form.values(),
        json!({ "model": "whisper-1", "file": "<file:a.mp3>" })
    );
    let response = post_form(
        &transport(),
        url(&server, "/upload"),
        Headers::new(),
        form,
        &handlers(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(response.value.id, "f");
}

#[tokio::test]
async fn transport_does_not_follow_redirects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/elsewhere"))
        .mount(&server)
        .await;
    let response = transport()
        .execute(HttpRequest::get(url(&server, "/r")).with_body(RequestBody::Empty))
        .await
        .unwrap();
    assert_eq!(response.status, StatusCode::FOUND);
    assert_eq!(response.headers.get_str("location"), Some("/elsewhere"));
}
