use std::sync::Arc;
use std::time::Duration;

use ferrin_policy::HttpPolicyClient;
use ferrin_policy::PolicyClient;
use ferrin_policy::PolicyError;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_testing::Fixture;
use ferrin_testing::FixtureServer;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

async fn client(server: &FixtureServer) -> HttpPolicyClient {
    HttpPolicyClient::builder(server.url())
        .url_policy(UrlPolicy::new().allow_http().allow_private_networks())
        .headers(Headers::new().with("authorization", "Bearer test-token"))
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[tokio::test]
async fn posts_the_input_and_returns_the_result() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::POST,
        "/v1/data/ferrin/tools/decision",
        Fixture::json(&json!({ "result": { "decision": "allow", "reason": "ok" } })),
    );
    let client = client(&server).await;
    let result = client
        .evaluate("ferrin.tools.decision", json!({ "tool": { "name": "x" } }))
        .await
        .unwrap();
    assert_eq!(result, json!({ "decision": "allow", "reason": "ok" }));

    let received = server.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].path, "/v1/data/ferrin/tools/decision");
    assert_eq!(
        received[0].body_json().unwrap(),
        json!({ "input": { "tool": { "name": "x" } } })
    );
    assert_eq!(received[0].header("content-type"), Some("application/json"));
    assert_eq!(
        received[0].header("authorization"),
        Some("Bearer test-token")
    );
    assert!(
        received[0]
            .header("user-agent")
            .is_some_and(|agent| agent.contains("ferrin-policy/")),
        "{:?}",
        received[0].headers
    );
}

#[tokio::test]
async fn missing_results_are_null() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(Method::POST, "/v1/data/p", Fixture::json(&json!({})));
    let result = client(&server).await.evaluate("/data/p", json!({})).await;
    assert_eq!(result.unwrap(), JsonValue::Null);
}

#[tokio::test]
async fn reports_status_and_body_errors() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::POST,
        "/v1/data/broken",
        Fixture::json_status(StatusCode::INTERNAL_SERVER_ERROR, &json!({ "code": "x" })),
    );
    server.mount(
        Method::POST,
        "/v1/data/text",
        Fixture::complete(StatusCode::OK, "text/plain", "not json"),
    );
    server.mount(
        Method::POST,
        "/v1/data/array",
        Fixture::json(&json!([1, 2])),
    );
    let client = client(&server).await;
    match client.evaluate("broken", json!({})).await.unwrap_err() {
        PolicyError::Status { status, body } => {
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(body, r#"{"code":"x"}"#);
        }
        other => panic!("unexpected error {other:?}"),
    }
    assert!(matches!(
        client.evaluate("text", json!({})).await.unwrap_err(),
        PolicyError::InvalidResponse { .. }
    ));
    assert!(matches!(
        client.evaluate("array", json!({})).await.unwrap_err(),
        PolicyError::InvalidResponse { .. }
    ));
    assert!(matches!(
        client.evaluate("", json!({})).await.unwrap_err(),
        PolicyError::InvalidPath { .. }
    ));
    assert!(matches!(
        client.evaluate("a//b", json!({})).await.unwrap_err(),
        PolicyError::InvalidPath { .. }
    ));
}

#[tokio::test]
async fn the_default_url_policy_rejects_local_http_servers() {
    let server = FixtureServer::start().await.unwrap();
    let client = HttpPolicyClient::new(server.url()).unwrap();
    assert!(matches!(
        client.evaluate("p", json!({})).await.unwrap_err(),
        PolicyError::InvalidUrl { .. }
    ));
    assert!(server.received().is_empty());
}

#[tokio::test]
async fn base_paths_are_preserved() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::POST,
        "/opa/v1/data/p",
        Fixture::json(&json!({ "result": true })),
    );
    let base = server.url().join("opa/").unwrap();
    let client = Arc::new(
        HttpPolicyClient::builder(base)
            .url_policy(UrlPolicy::new().allow_http().allow_private_networks())
            .build()
            .unwrap(),
    );
    assert_eq!(client.evaluate("p", json!({})).await.unwrap(), json!(true));
    assert_eq!(server.received()[0].path, "/opa/v1/data/p");
}
