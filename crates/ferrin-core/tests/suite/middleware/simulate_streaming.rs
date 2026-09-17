use ferrin_core::middleware::builtin::simulate_parts;
use ferrin_core::middleware::builtin::simulate_streaming;
use ferrin_spec::Content;
use ferrin_spec::FinishReason;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::GenerateResult;
use ferrin_testing::MockLanguageModel;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::render;
use super::common::stream;
use super::common::wrapped;

fn result() -> GenerateResult {
    let mut result = GenerateResult::new(
        vec![
            Content::reasoning("thinking"),
            Content::text(""),
            Content::text("answer"),
            Content::ToolCall(ToolCall::new("c1", "f", "{}")),
        ],
        FinishReason::tool_calls(),
    );
    result.usage = Usage::totals(4, 6);
    result.warnings = vec![Warning::other("w")];
    result.response.id = Some("r1".to_owned());
    result
}

#[test]
fn expands_a_complete_result_into_parts() {
    let parts = simulate_parts(&result());
    assert_eq!(
        render(&parts),
        vec![
            "stream-start",
            "response-metadata",
            "reasoning-start(0)",
            "reasoning-delta(0):thinking",
            "reasoning-end(0)",
            "text-start(1)",
            "text-delta(1):answer",
            "text-end(1)",
            "tool-call",
            "finish",
        ]
    );
    match &parts[0] {
        StreamPart::StreamStart { warnings } => assert_eq!(warnings.len(), 1),
        other => panic!("unexpected {other:?}"),
    }
    match &parts[1] {
        StreamPart::ResponseMetadata { id, .. } => assert_eq!(id.as_deref(), Some("r1")),
        other => panic!("unexpected {other:?}"),
    }
    match parts.last().unwrap() {
        StreamPart::Finish {
            finish_reason,
            usage,
            ..
        } => {
            assert_eq!(*finish_reason, FinishReason::tool_calls());
            assert_eq!(*usage, Usage::totals(4, 6));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn wrap_stream_uses_do_generate() {
    let mock = MockLanguageModel::builder()
        .generate(result())
        .build_shared();
    let model = wrapped(mock.clone(), simulate_streaming());
    let parts = stream(&model).await;
    assert_eq!(parts.len(), 10);
    assert_eq!(mock.generate_calls().len(), 1);
    assert!(mock.stream_calls().is_empty());
}

#[test]
fn text_and_reasoning_metadata_follow_reference_event_boundaries() {
    let metadata = Some(
        [(
            "mock".to_owned(),
            json!({"signature":"value"}).as_object().unwrap().clone(),
        )]
        .into(),
    );
    let result = GenerateResult::new(
        vec![
            Content::Text {
                text: "answer".into(),
                provider_metadata: metadata.clone(),
            },
            Content::Reasoning {
                text: "thinking".into(),
                provider_metadata: metadata,
            },
        ],
        FinishReason::stop(),
    );
    let parts = simulate_parts(&result);
    assert_eq!(
        serde_json::to_value(&parts[2..8]).unwrap(),
        json!([
            {"type":"text-start","id":"0"},
            {"type":"text-delta","id":"0","delta":"answer"},
            {"type":"text-end","id":"0"},
            {"type":"reasoning-start","id":"1","provider_metadata":{"mock":{"signature":"value"}}},
            {"type":"reasoning-delta","id":"1","delta":"thinking"},
            {"type":"reasoning-end","id":"1"}
        ])
    );
}
