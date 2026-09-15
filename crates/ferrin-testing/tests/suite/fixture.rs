use std::time::Duration;

use bytes::Bytes;
use ferrin_provider_util::HttpRequest;
use ferrin_provider_util::HttpTransport;
use ferrin_provider_util::RequestBody;
use ferrin_provider_util::ReqwestTransport;
use ferrin_testing::Fixture;
use ferrin_testing::FixtureBody;
use ferrin_testing::FixtureServer;
use ferrin_testing::fixture::decode_events_file;
use ferrin_testing::fixture::encode_events_file;
use futures_util::StreamExt;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

async fn body_chunks(response: ferrin_provider_util::HttpResponse) -> Vec<Bytes> {
    response.body.map(|chunk| chunk.unwrap()).collect().await
}

fn concat(chunks: &[Bytes]) -> String {
    let mut out = Vec::new();
    for chunk in chunks {
        out.extend_from_slice(chunk);
    }
    String::from_utf8(out).unwrap()
}

#[tokio::test]
async fn serves_json_fixtures_and_records_requests() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::POST,
        "/v1/chat",
        Fixture::json(&json!({"ok": true})).with_header("x-request-id", "abc"),
    );
    let transport = ReqwestTransport::new().unwrap();
    let request = HttpRequest::post(server.url().join("v1/chat?x=1").unwrap())
        .with_headers(ferrin_spec::Headers::new().with("content-type", "application/json"))
        .with_body(RequestBody::json(Bytes::from_static(b"{\"q\":\"hi\"}")));
    let response = transport.execute(request).await.unwrap();
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.headers.get_str("content-type"),
        Some("application/json")
    );
    assert_eq!(response.headers.get_str("x-request-id"), Some("abc"));
    assert_eq!(concat(&body_chunks(response).await), "{\"ok\":true}");

    let received = server.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].method, Method::POST);
    assert_eq!(received[0].path, "/v1/chat");
    assert_eq!(received[0].query.as_deref(), Some("x=1"));
    assert_eq!(received[0].body_json().unwrap(), json!({"q": "hi"}));
    assert_eq!(received[0].header("content-type"), Some("application/json"));
}

#[tokio::test]
async fn unmatched_requests_get_404_and_once_routes_are_consumed() {
    let server = FixtureServer::start().await.unwrap();
    server.mount_once(
        Method::GET,
        "/once",
        Fixture::complete(StatusCode::OK, "text/plain", "hi"),
    );
    let transport = ReqwestTransport::new().unwrap();

    let first = transport
        .execute(HttpRequest::get(server.url().join("once").unwrap()))
        .await
        .unwrap();
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(concat(&body_chunks(first).await), "hi");

    let second = transport
        .execute(HttpRequest::get(server.url().join("once").unwrap()))
        .await
        .unwrap();
    assert_eq!(second.status, StatusCode::NOT_FOUND);
    assert_eq!(
        concat(&body_chunks(second).await),
        "{\"error\":\"no fixture mounted for GET /once\"}"
    );
    assert_eq!(server.received_count(), 2);
    server.reset();
    assert_eq!(server.received_count(), 0);
}

#[tokio::test]
async fn streams_events_as_separate_frames() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::POST,
        "/stream",
        Fixture::sse([
            "data: {\"n\":1}",
            "event: x\ndata: {\"n\":2}",
            "data: [DONE]",
        ])
        .with_chunk_delay(Duration::from_millis(30)),
    );
    let transport = ReqwestTransport::new().unwrap();
    let response = transport
        .execute(HttpRequest::post(server.url().join("stream").unwrap()))
        .await
        .unwrap();
    assert_eq!(
        response.headers.get_str("content-type"),
        Some("text/event-stream")
    );
    let chunks = body_chunks(response).await;
    assert!(
        chunks.len() >= 2,
        "expected separate frames, got {chunks:?}"
    );
    assert_eq!(
        concat(&chunks),
        "data: {\"n\":1}\n\nevent: x\ndata: {\"n\":2}\n\ndata: [DONE]\n\n"
    );
}

#[test]
fn events_file_round_trip() {
    let events = ["data: a", "event: x\ndata: b\\c", "data: \r"];
    let encoded = encode_events_file(events);
    assert_eq!(encoded, "data: a\nevent: x\\ndata: b\\\\c\ndata: \\r\n");
    assert_eq!(decode_events_file(&encoded), events);
    assert_eq!(decode_events_file("\n\ndata: only\n"), vec!["data: only"]);
}

#[test]
fn loads_fixtures_from_files() {
    let dir = std::env::temp_dir().join(format!(
        "ferrin-testing-fixture-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("text.response.json"), "{\"a\":1}").unwrap();
    std::fs::write(
        dir.join("text.meta.json"),
        "{\"status\":429,\"headers\":{\"retry-after\":\"3\"},\"recorded_at\":\"x\"}",
    )
    .unwrap();
    std::fs::write(dir.join("chunks.chunks.txt"), "data: 1\ndata: 2\n").unwrap();

    let fixture = Fixture::load(&dir, "text").unwrap();
    assert_eq!(fixture.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(fixture.headers.get_str("retry-after"), Some("3"));
    assert_eq!(
        fixture.headers.get_str("content-type"),
        Some("application/json")
    );
    match fixture.body {
        FixtureBody::Complete(bytes) => assert_eq!(bytes, Bytes::from_static(b"{\"a\":1}")),
        other => panic!("unexpected body {other:?}"),
    }

    let fixture = Fixture::load(&dir, "chunks").unwrap();
    assert_eq!(
        fixture.headers.get_str("content-type"),
        Some("text/event-stream")
    );
    match fixture.body {
        FixtureBody::Events { events, .. } => assert_eq!(events, vec!["data: 1", "data: 2"]),
        other => panic!("unexpected body {other:?}"),
    }

    let error = Fixture::load(&dir, "missing").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn finite_routes_respect_zero_one_and_multiple_counts() {
    for times in [0, 1, 3] {
        let server = FixtureServer::start().await.unwrap();
        server.mount_times(
            Method::GET,
            "/count",
            Fixture::complete(StatusCode::OK, "text/plain", "limited"),
            times,
        );
        server.mount(
            Method::GET,
            "/count",
            Fixture::complete(StatusCode::OK, "text/plain", "fallback"),
        );
        let transport = ReqwestTransport::new().unwrap();
        let mut bodies = Vec::new();
        for _ in 0..=times {
            let response = transport
                .execute(HttpRequest::get(server.url().join("count").unwrap()))
                .await
                .unwrap();
            bodies.push(concat(&body_chunks(response).await));
        }
        let mut expected = vec!["limited"; times];
        expected.push("fallback");
        assert_eq!(bodies, expected);
    }
}
