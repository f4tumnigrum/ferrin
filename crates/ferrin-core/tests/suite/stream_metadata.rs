use ferrin_core::StepContent;
use ferrin_core::StreamEvent;
use ferrin_core::stream_text;
use ferrin_core::stream_text::smooth_stream;
use ferrin_core::stream_text::transforms::SmoothStreamConfig;
use ferrin_spec::FinishReason;
use ferrin_spec::PartId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;

fn metadata(value: &str) -> Option<ProviderMetadata> {
    Some(serde_json::from_value(json!({"provider":{"marker":value}})).unwrap())
}

#[tokio::test]
async fn smoothing_keeps_metadata_with_its_delta_and_preserves_empty_deltas() {
    let parts = vec![
        StreamPart::stream_start(),
        StreamPart::TextStart {
            id: PartId::new("text"),
            provider_metadata: None,
        },
        StreamPart::TextDelta {
            id: PartId::new("text"),
            delta: "first ".into(),
            provider_metadata: metadata("first"),
        },
        StreamPart::TextDelta {
            id: PartId::new("text"),
            delta: "buffer".into(),
            provider_metadata: metadata("buffer"),
        },
        StreamPart::TextDelta {
            id: PartId::new("text"),
            delta: "next".into(),
            provider_metadata: metadata("next"),
        },
        StreamPart::TextDelta {
            id: PartId::new("text"),
            delta: String::new(),
            provider_metadata: metadata("empty"),
        },
        StreamPart::TextEnd {
            id: PartId::new("text"),
            provider_metadata: None,
        },
        StreamPart::ReasoningStart {
            id: PartId::new("reason"),
            provider_metadata: None,
        },
        StreamPart::ReasoningDelta {
            id: PartId::new("reason"),
            delta: "reason".into(),
            provider_metadata: metadata("reason"),
        },
        StreamPart::ReasoningEnd {
            id: PartId::new("reason"),
            provider_metadata: None,
        },
        StreamPart::finish(FinishReason::stop(), Usage::default()),
    ];
    let (events, completion) = stream_text(mock().stream(parts).build_shared())
        .prompt("hi")
        .transform(smooth_stream(SmoothStreamConfig::new().delay(None)))
        .await
        .unwrap()
        .split();
    let deltas: Vec<_> = events
        .filter_map(|event| async move {
            match event {
                StreamEvent::TextDelta {
                    id,
                    text,
                    provider_metadata,
                } => Some(("text", id, text, provider_metadata)),
                StreamEvent::ReasoningDelta {
                    id,
                    text,
                    provider_metadata,
                } => Some(("reason", id, text, provider_metadata)),
                _ => None,
            }
        })
        .collect()
        .await;
    assert_eq!(
        deltas,
        vec![
            (
                "text",
                PartId::new("text"),
                "first ".into(),
                metadata("first")
            ),
            (
                "text",
                PartId::new("text"),
                "buffer".into(),
                metadata("buffer")
            ),
            ("text", PartId::new("text"), "next".into(), metadata("next")),
            (
                "text",
                PartId::new("text"),
                String::new(),
                metadata("empty")
            ),
            (
                "reason",
                PartId::new("reason"),
                "reason".into(),
                metadata("reason")
            ),
        ]
    );
    let result = completion.await.unwrap();
    assert_eq!(
        result.last_step().content,
        vec![
            StepContent::Text {
                text: "first buffernext".into(),
                provider_metadata: metadata("empty")
            },
            StepContent::Reasoning {
                text: "reason".into(),
                provider_metadata: metadata("reason")
            },
        ]
    );
}
