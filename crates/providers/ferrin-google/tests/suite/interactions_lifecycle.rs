//! Interactions polling, resumption, cancellation and error boundaries.

use ferrin_google::GoogleInteractionOptions;
use ferrin_spec::CallOptions;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::error::ProviderError;
use ferrin_testing::Fixture;
use futures_util::StreamExt;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::google_options;

fn background() -> CallOptions {
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Research")]);
    options.provider_options = google_options(json!({"agent":"deep-research", "background":true}));
    options
}

fn mount_start(test: &TestProvider) {
    test.mount_fixture(
        Method::POST,
        "/v1beta/interactions",
        Fixture::json(&json!({"id":"job-1", "status":"in_progress"})),
    );
}

#[tokio::test]
async fn resumption_uses_event_id_and_deduplicates_boundary_delta() {
    let test = TestProvider::start().await;
    mount_start(&test);
    let start = json!({"event_id":"event-1","event_type":"step.start","index":0,"step":{"type":"model_output"}});
    let first = json!({"event_id":"event-2","event_type":"step.delta","index":0,"delta":{"type":"text","text":"Hello"}});
    test.server.mount_once(
        Method::GET,
        "/v1beta/interactions/job-1",
        Fixture::sse_json(&[start, first.clone()]),
    );
    test.mount_fixture(Method::GET, "/v1beta/interactions/job-1", Fixture::sse_json(&[
        first,
        json!({"event_id":"event-3","event_type":"step.delta","index":0,"delta":{"type":"text","text":" world"}}),
        json!({"event_id":"event-4","event_type":"step.stop","index":0}),
        json!({"event_id":"event-5","event_type":"interaction.completed","interaction":{"id":"job-1","status":"completed"}}),
    ]));
    let parts = collect_checked(
        test.provider
            .interactions("deep-research")
            .do_stream(background())
            .await
            .unwrap(),
    )
    .await;
    let text: String = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello world");
    let requests = test.server.received();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.query.as_deref())
            .collect::<Vec<_>>(),
        vec![
            None,
            Some("stream=true"),
            Some("stream=true&last_event_id=event-2")
        ]
    );
    assert!(matches!(parts.last(), Some(StreamPart::Finish { .. })));
}

#[tokio::test]
async fn explicit_abort_cancels_remote_background_job() {
    let test = TestProvider::start().await;
    mount_start(&test);
    test.mount_fixture(Method::GET, "/v1beta/interactions/job-1", Fixture::sse_json(&[json!({"event_type":"interaction.created", "interaction":{"id":"job-1","status":"in_progress"}})]).hold_open());
    test.mount_fixture(
        Method::POST,
        "/v1beta/interactions/job-1/cancel",
        Fixture::json(&json!({"id":"job-1","status":"cancelled"})),
    );
    let options = background();
    let cancellation = options.cancellation.clone();
    let mut result = test
        .provider
        .interactions("deep-research")
        .do_stream(options)
        .await
        .unwrap();
    assert!(matches!(
        result.stream.next().await,
        Some(StreamPart::StreamStart { .. })
    ));
    assert!(matches!(
        result.stream.next().await,
        Some(StreamPart::ResponseMetadata { .. })
    ));
    cancellation.cancel();
    assert!(matches!(
        result.stream.next().await,
        Some(StreamPart::Error { .. })
    ));
    assert!(result.stream.next().await.is_none());
    assert_eq!(
        test.server
            .received()
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/v1beta/interactions",
            "/v1beta/interactions/job-1",
            "/v1beta/interactions/job-1/cancel"
        ]
    );
}

#[tokio::test]
async fn background_generate_polls_then_returns_final_content() {
    let test = TestProvider::start().await;
    mount_start(&test);
    test.mount(
        Method::GET,
        "/v1beta/interactions/job-1",
        "interactions",
        "basic",
    );
    let result = test
        .provider
        .interactions("deep-research")
        .do_generate(background())
        .await
        .unwrap();
    assert_eq!(result.response.id.as_deref(), Some("interaction-1"));
    assert_eq!(test.server.received_count(), 2);
}

#[tokio::test]
async fn already_terminal_background_post_needs_no_get() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/interactions",
        "interactions",
        "basic",
    );
    let parts = collect_checked(
        test.provider
            .interactions("deep-research")
            .do_stream(background())
            .await
            .unwrap(),
    )
    .await;
    assert!(matches!(parts.last(), Some(StreamPart::Finish { .. })));
    assert_eq!(test.server.received_count(), 1);
}

#[tokio::test]
async fn missing_background_id_and_unresumable_eof_fail() {
    for response in [
        json!({"status":"in_progress"}),
        json!({"id":"job-1","status":"in_progress"}),
    ] {
        let test = TestProvider::start().await;
        test.mount_fixture(
            Method::POST,
            "/v1beta/interactions",
            Fixture::json(&response),
        );
        test.mount_fixture(
            Method::GET,
            "/v1beta/interactions/job-1",
            Fixture::sse(Vec::<String>::new()),
        );
        let result = test
            .provider
            .interactions("deep-research")
            .do_stream(background())
            .await;
        if response.get("id").is_none() {
            assert!(matches!(result, Err(ProviderError::InvalidResponseData(_))));
        } else {
            let parts = collect_checked(result.unwrap()).await;
            assert!(matches!(parts.last(), Some(StreamPart::Error { .. })));
            assert_eq!(test.server.received_count(), 2);
        }
    }
}

#[tokio::test]
async fn interaction_resource_operations_encode_server_ids() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::GET,
        "/v1beta/interactions/job%2Fpart%3Fquery=1",
        Fixture::json(&json!({"status":"completed"})),
    );
    test.mount_fixture(
        Method::POST,
        "/v1beta/interactions/job%2Fpart%3Fquery=1/cancel",
        Fixture::json(&json!({"status":"cancelled"})),
    );
    let model = test.provider.interactions("gemini-2.5-flash");
    assert_eq!(
        model
            .get_interaction("job/part?query=1", GoogleInteractionOptions::default())
            .await
            .unwrap(),
        json!({"status":"completed"})
    );
    assert_eq!(
        model
            .cancel_interaction("job/part?query=1", GoogleInteractionOptions::default())
            .await
            .unwrap(),
        json!({"status":"cancelled"})
    );
    assert!(
        model
            .get_interaction("..", GoogleInteractionOptions::default())
            .await
            .is_err()
    );
    assert_eq!(test.server.received_count(), 2);
}

#[tokio::test]
async fn malformed_terminal_and_invalid_function_json_emit_errors() {
    let cases = [
        vec![json!({"event_type":"interaction.completed","interaction":{"status":"in_progress"}})],
        vec![
            json!({"event_type":"step.start","index":0,"step":{"type":"function_call","id":"call-1","name":"weather"}}),
            json!({"event_type":"step.delta","index":0,"delta":{"type":"arguments_delta","arguments":"{"}}),
            json!({"event_type":"step.stop","index":0}),
        ],
        vec![json!({"event_type":"step.delta","index":0,"delta":{"type":"text","text":"orphan"}})],
        vec![
            json!({"event_type":"step.start","index":0,"step":{"type":"model_output"}}),
            json!({"event_type":"error","error":{"message":"provider failure","code":500}}),
        ],
    ];
    for events in cases {
        let test = TestProvider::start().await;
        test.mount_fixture(
            Method::POST,
            "/v1beta/interactions",
            Fixture::sse_json(&events),
        );
        let parts = collect_checked(
            test.provider
                .interactions("gemini-2.5-flash")
                .do_stream(CallOptions::new(vec![PromptMessage::user_text("Hello")]))
                .await
                .unwrap(),
        )
        .await;
        assert!(matches!(parts.last(), Some(StreamPart::Error { .. })));
        assert!(
            !parts
                .iter()
                .any(|part| matches!(part, StreamPart::ToolCall(_) | StreamPart::Finish { .. }))
        );
    }
}

#[tokio::test]
async fn http_errors_preserve_retryability() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1beta/interactions",
        Fixture::json_status(
            StatusCode::TOO_MANY_REQUESTS,
            &json!({"error":{"message":"rate limited","code":429,"status":"RESOURCE_EXHAUSTED"}}),
        )
        .with_header("retry-after", "1"),
    );
    let error = test
        .provider
        .interactions("gemini-2.5-flash")
        .do_generate(CallOptions::default())
        .await
        .unwrap_err();
    assert!(error.is_retryable());
    assert_eq!(error.status_code(), Some(StatusCode::TOO_MANY_REQUESTS));
}
