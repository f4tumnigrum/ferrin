//! Stream boundary regressions for initial payloads and malformed frames.

use ferrin_spec::CallOptions;
use ferrin_spec::LanguageModel;
use ferrin_spec::StreamPart;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;

#[tokio::test]
async fn initial_payloads_arguments_and_sources_survive_later_deltas() {
    let test = TestProvider::start().await;
    let annotation = json!({"type":"url_citation","url":"https://example.com","title":"Example"});
    test.mount_fixture(Method::POST, "/v1beta/interactions", Fixture::sse_json(&[
        json!({"event_type":"step.start","index":0,"step":{"type":"thought","summary":[{"type":"text","text":"Initial thought"}]}}),
        json!({"event_type":"step.delta","index":0,"delta":{"type":"thought_summary","content":{"type":"text","text":" and more"}}}),
        json!({"event_type":"step.stop","index":0}),
        json!({"event_type":"step.start","index":1,"step":{"type":"model_output","content":[{"type":"text","text":"Initial text","annotations":[annotation.clone()]}]}}),
        json!({"event_type":"step.delta","index":1,"delta":{"type":"text","text":" and more"}}),
        json!({"event_type":"step.delta","index":1,"delta":{"type":"text_annotation","annotations":[annotation]}}),
        json!({"event_type":"step.stop","index":1}),
        json!({"event_type":"step.start","index":2,"step":{"type":"function_call","id":"call-1","name":"weather","arguments":{}}}),
        json!({"event_type":"step.delta","index":2,"delta":{"type":"arguments_delta","arguments":"{\"city\":\"Paris\"}"}}),
        json!({"event_type":"step.stop","index":2}),
        json!({"event_type":"interaction.completed","interaction":{"status":"requires_action"}}),
    ]).hold_open());
    let result = test
        .provider
        .interactions("gemini-2.5-flash")
        .do_stream(CallOptions::default())
        .await
        .unwrap();
    let parts = tokio::time::timeout(std::time::Duration::from_secs(2), collect_checked(result))
        .await
        .unwrap();
    let texts: String = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    let reasoning: String = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::ReasoningDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    let calls: Vec<_> = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::ToolCall(call) => Some((call.tool_name.as_str(), call.input.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        (texts, reasoning, calls),
        (
            "Initial text and more".to_owned(),
            "Initial thought and more".to_owned(),
            vec![("weather", "{\"city\":\"Paris\"}")]
        )
    );
    assert_eq!(
        parts
            .iter()
            .filter(|part| matches!(part, StreamPart::Source(_)))
            .count(),
        1
    );
    assert!(matches!(parts.last(), Some(StreamPart::Finish { .. })));
}

#[tokio::test]
async fn malformed_objects_and_mismatched_deltas_never_panic_or_execute() {
    let cases = [
        vec![
            json!({"event_type":"step.start","index":0,"step":{"type":"function_call","id":"call-1","name":"weather"}}),
            json!({"event_type":"step.delta","index":0,"delta":{"type":"arguments_delta","arguments":{"bad":"shape"}}}),
            json!({"event_type":"step.stop","index":0}),
        ],
        vec![
            json!({"event_type":"step.start","index":0,"step":"invalid"}),
            json!({"event_type":"step.delta","index":0,"delta":{"type":"thought_signature","signature":"fixture"}}),
        ],
        vec![
            json!({"event_type":"step.start","index":0,"step":{"type":"function_call","id":"call-1","name":"weather"}}),
            json!({"event_type":"step.delta","index":0,"delta":{"type":"text","text":"invalid"}}),
        ],
        vec![
            json!({"event_type":"step.start","index":0,"step":{"type":"model_output"}}),
            json!({"event_type":"step.delta","index":0,"delta":"invalid"}),
        ],
        vec![json!({"event_type":"interaction.completed","interaction":null})],
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
                .do_stream(CallOptions::default())
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
async fn unknown_delta_does_not_change_function_discriminant() {
    let test = TestProvider::start().await;
    test.mount_fixture(Method::POST, "/v1beta/interactions", Fixture::sse_json(&[
        json!({"event_type":"step.start","index":0,"step":{"type":"function_call","id":"call-1","name":"weather","arguments":{"city":"Paris"}}}),
        json!({"event_type":"step.delta","index":0,"delta":{"type":"future_delta","content":"extra"}}),
        json!({"event_type":"step.stop","index":0}),
        json!({"event_type":"interaction.completed","interaction":{"status":"requires_action"}}),
    ]));
    let parts = collect_checked(
        test.provider
            .interactions("gemini-2.5-flash")
            .do_stream(CallOptions::default())
            .await
            .unwrap(),
    )
    .await;
    let calls: Vec<_> = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::ToolCall(call) => Some((call.tool_call_id.as_str(), call.input.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(calls, vec![("call-1", "{\"city\":\"Paris\"}")]);
}
