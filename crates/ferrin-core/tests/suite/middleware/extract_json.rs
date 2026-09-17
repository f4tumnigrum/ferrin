use ferrin_core::middleware::builtin::extract_json;
use ferrin_core::middleware::builtin::strip_json_fences;
use ferrin_spec::StreamPart;
use ferrin_testing::MockLanguageModel;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::generate;
use super::common::render;
use super::common::render_content;
use super::common::stream;
use super::common::text_result;
use super::common::text_stream;
use super::common::wrapped;

fn joined_text(parts: &[StreamPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn strips_fences() {
    assert_eq!(strip_json_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
    assert_eq!(strip_json_fences("```\n{\"a\":1}\n```  \n"), "{\"a\":1}");
    assert_eq!(strip_json_fences("  {\"a\":1}  "), "{\"a\":1}");
    assert_eq!(strip_json_fences("```json{\"a\":1}```"), "{\"a\":1}");
}

#[test]
fn fence_whitespace_matches_ecmascript_trim_rules() {
    assert_eq!(
        strip_json_fences("```json\u{feff}\n{\"a\":1}\n```\u{feff}"),
        "{\"a\":1}"
    );
    assert_eq!(strip_json_fences("\u{feff}{\"a\":1}\u{feff}"), "{\"a\":1}");
    assert_eq!(
        strip_json_fences("\u{0085}{\"a\":1}\u{0085}"),
        "\u{0085}{\"a\":1}\u{0085}"
    );
}

#[tokio::test]
async fn unmatched_text_deltas_preserve_metadata() {
    let parts = vec![StreamPart::TextDelta {
        id: "unmatched".into(),
        delta: "raw".into(),
        provider_metadata: Some(
            [(
                "mock".to_owned(),
                json!({"signature":"value"}).as_object().unwrap().clone(),
            )]
            .into(),
        ),
    }];
    let expected = serde_json::to_value(&parts).unwrap();
    let model = wrapped(
        MockLanguageModel::builder().stream(parts).build_shared(),
        extract_json(),
    );
    assert_eq!(
        serde_json::to_value(stream(&model).await).unwrap(),
        expected
    );
}

#[tokio::test]
async fn generate_strips_fences_from_text_parts() {
    let model = wrapped(
        MockLanguageModel::builder()
            .generate(text_result("```json\n{\"a\": 1}\n```"))
            .build_shared(),
        extract_json(),
    );
    assert_eq!(
        render_content(&generate(&model).await.content),
        vec!["text:{\"a\": 1}"]
    );
}

#[tokio::test]
async fn stream_strips_fences_incrementally() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream(
                "1",
                &[
                    "```json\n{\"city\": \"Berlin\", ",
                    "\"temperature\": 21}\n```",
                ],
            ))
            .build_shared(),
        extract_json(),
    );
    let parts = stream(&model).await;
    let rendered = render(&parts);
    assert_eq!(rendered[0], "stream-start");
    assert_eq!(rendered[1], "text-start(1)");
    assert_eq!(rendered[rendered.len() - 2], "text-end(1)");
    assert_eq!(rendered[rendered.len() - 1], "finish");
    assert_eq!(
        joined_text(&parts),
        "{\"city\": \"Berlin\", \"temperature\": 21}"
    );
    // Text streamed before the end: at least two deltas.
    assert!(
        rendered
            .iter()
            .filter(|r| r.starts_with("text-delta"))
            .count()
            >= 2
    );
}

#[tokio::test]
async fn stream_without_fences_passes_text_through() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream("1", &["{\"a\": ", "1}"]))
            .build_shared(),
        extract_json(),
    );
    let parts = stream(&model).await;
    assert_eq!(joined_text(&parts), "{\"a\": 1}");
    assert_eq!(render(&parts)[1], "text-start(1)");
}

#[tokio::test]
async fn custom_transform_buffers_the_whole_part() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream("1", &["abc", "def"]))
            .build_shared(),
        extract_json().transform(str::to_uppercase),
    );
    assert_eq!(
        render(&stream(&model).await),
        vec![
            "stream-start",
            "text-start(1)",
            "text-delta(1):ABCDEF",
            "text-end(1)",
            "finish",
        ]
    );
}
