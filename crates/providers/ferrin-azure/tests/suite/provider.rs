use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use ferrin_azure::AzureSettings;
use ferrin_azure::AzureUrlMode;
use ferrin_azure::create_azure;
use ferrin_azure::token_provider;
use ferrin_spec::CallOptions;
use ferrin_spec::EmbeddingModel;
use ferrin_spec::Headers;
use ferrin_spec::LanguageModel;
use ferrin_spec::Prompt;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_testing::Fixture;
use ferrin_testing::FixtureServer;
use futures_util::StreamExt;
use http::Method;
use pretty_assertions::assert_eq;
use secrecy::SecretString;
use serde_json::json;

fn settings(server: &FixtureServer) -> AzureSettings {
    AzureSettings {
        base_url: Some(server.url().join("openai/v1").unwrap()),
        api_key: Some(SecretString::from("test-azure-key")),
        ..AzureSettings::default()
    }
}

fn response() -> serde_json::Value {
    json!({"id":"resp_test", "created_at":1, "model":"gpt-test", "status":"completed", "output":[{"type":"message", "id":"msg_test", "role":"assistant", "content":[{"type":"output_text", "text":"hello", "annotations":[]}]}], "usage":{"input_tokens":2,"output_tokens":1}})
}

#[tokio::test]
async fn responses_and_embeddings_preserve_routing_auth_and_bodies() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::POST,
        "/openai/v1/responses",
        Fixture::json(&response()),
    );
    server.mount(
        Method::POST,
        "/openai/v1/embeddings",
        Fixture::json(&json!({"data":[{"embedding":[0.25,0.75]}],"usage":{"prompt_tokens":2}})),
    );
    let provider = create_azure(settings(&server)).unwrap();
    let model = provider.responses("deployment-main");
    let result = model
        .do_generate(CallOptions::new(Prompt::default()))
        .await
        .unwrap();
    assert_eq!(
        result.content[0].clone(),
        ferrin_spec::Content::Text {
            text: "hello".to_owned(),
            provider_metadata: Some(
                [(
                    "openai".to_owned(),
                    serde_json::from_value(json!({"itemId":"msg_test"})).unwrap()
                )]
                .into_iter()
                .collect()
            ),
        }
    );
    let embedded = provider
        .embedding("embedding-deployment")
        .do_embed(EmbedOptions::new(vec!["hello".to_owned()]))
        .await
        .unwrap();
    assert_eq!(embedded.embeddings, vec![vec![0.25, 0.75]]);
    let requests = server.received();
    assert_eq!(
        requests
            .iter()
            .map(|request| (
                request.path.as_str(),
                request.query.as_deref(),
                request.header("api-key"),
                request.header("authorization")
            ))
            .collect::<Vec<_>>(),
        vec![
            ("/openai/v1/responses", None, Some("test-azure-key"), None),
            ("/openai/v1/embeddings", None, Some("test-azure-key"), None)
        ]
    );
    assert_eq!(requests[0].body_json().unwrap()["model"], "deployment-main");
    assert_eq!(model.provider().as_str(), "azure.responses");
    assert!(
        requests[0]
            .header("user-agent")
            .unwrap()
            .contains("ferrin-azure/")
    );
    insta::assert_json_snapshot!("responses_request", requests[0].body_json().unwrap());
}

#[tokio::test]
async fn legacy_deployments_route_model_and_api_version() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::POST,
        "/openai/deployments/deploy%2D1/responses",
        Fixture::json(&response()),
    );
    let provider = create_azure(AzureSettings {
        base_url: Some(server.url().join("openai").unwrap()),
        url_mode: AzureUrlMode::Deployment,
        api_version: Some("2025-04-01-preview".to_owned()),
        ..settings(&server)
    })
    .unwrap();
    provider
        .responses("deploy-1")
        .do_generate(CallOptions::new(Prompt::default()))
        .await
        .unwrap();
    let request = server.received().remove(0);
    assert_eq!(
        (request.path, request.query),
        (
            "/openai/deployments/deploy%2D1/responses".to_owned(),
            Some("api-version=2025-04-01-preview".to_owned())
        )
    );
}

#[tokio::test]
async fn entra_tokens_refresh_per_request_and_override_call_auth() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::POST,
        "/openai/v1/responses",
        Fixture::json(&response()),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let provider = create_azure(AzureSettings {
        api_key: None,
        token_provider: Some(token_provider(move || {
            let n = count.fetch_add(1, Ordering::SeqCst);
            async move { Ok(SecretString::from(format!("test-token-{n}"))) }
        })),
        ..settings(&server)
    })
    .unwrap();
    for _ in 0..2 {
        let mut options = CallOptions::new(Prompt::default());
        options.headers = Headers::new()
            .with("authorization", "Bearer stray")
            .with("api-key", "stray");
        provider
            .responses("deployment")
            .do_generate(options)
            .await
            .unwrap();
    }
    let requests = server.received();
    assert_eq!(
        requests
            .iter()
            .map(|r| (r.header("authorization"), r.header("api-key")))
            .collect::<Vec<_>>(),
        vec![
            (Some("Bearer test-token-0"), None),
            (Some("Bearer test-token-1"), None)
        ]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn conflicting_credentials_and_invalid_deployments_fail_before_http() {
    let server = FixtureServer::start().await.unwrap();
    assert!(
        create_azure(AzureSettings {
            token_provider: Some(token_provider(|| async { Ok(SecretString::from("token")) })),
            ..settings(&server)
        })
        .is_err()
    );
    let provider = create_azure(AzureSettings {
        url_mode: AzureUrlMode::Deployment,
        ..settings(&server)
    })
    .unwrap();
    for deployment in ["", ".", "..", "../../admin", "x/y"] {
        assert!(
            provider
                .responses(deployment)
                .do_generate(CallOptions::new(Prompt::default()))
                .await
                .is_err()
        );
    }
    assert!(server.received().is_empty());
}

#[tokio::test]
async fn token_failures_are_redacted_and_cancellation_interrupts_token_wait() {
    let server = FixtureServer::start().await.unwrap();
    let provider = create_azure(AzureSettings {
        api_key: None,
        token_provider: Some(token_provider(|| async {
            Err(ferrin_spec::error::ProviderError::other(
                std::io::Error::other("secret-credential-value"),
            ))
        })),
        ..settings(&server)
    })
    .unwrap();
    let error = provider
        .responses("deployment")
        .do_generate(CallOptions::new(Prompt::default()))
        .await
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains("secret-credential-value"));
    let provider = create_azure(AzureSettings {
        api_key: None,
        token_provider: Some(token_provider(std::future::pending)),
        ..settings(&server)
    })
    .unwrap();
    let options = CallOptions::new(Prompt::default());
    options.cancellation.cancel();
    assert!(
        provider
            .responses("deployment")
            .do_generate(options)
            .await
            .is_err()
    );
    assert!(server.received().is_empty());
}

#[tokio::test]
async fn responses_stream_uses_azure_auth_and_standard_events() {
    let server = FixtureServer::start().await.unwrap();
    let created = json!({"type":"response.created", "response":{"id":"resp_test","created_at":1,"model":"gpt-test"}});
    let done = json!({"type":"response.completed", "response":response()});
    server.mount(
        Method::POST,
        "/openai/v1/responses",
        Fixture::sse_json([&created, &done]),
    );
    let provider = create_azure(settings(&server)).unwrap();
    let parts: Vec<_> = provider
        .responses("deployment")
        .do_stream(CallOptions::new(Prompt::default()))
        .await
        .unwrap()
        .stream
        .collect()
        .await;
    assert!(matches!(
        parts.last(),
        Some(ferrin_spec::StreamPart::Finish { .. })
    ));
    assert_eq!(
        server.received()[0].header("api-key"),
        Some("test-azure-key")
    );
}
