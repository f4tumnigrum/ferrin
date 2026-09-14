//! Port of the reference `extractReasoningMiddleware` test suite (14 cases,
//! same names).

use ferrin_core::middleware::builtin::extract_reasoning;
use ferrin_spec::FinishReason;
use ferrin_spec::PartId;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::GenerateResult;
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

// ---- wrapGenerate ----

#[tokio::test]
async fn should_extract_reasoning_from_think_tags() {
    let model = wrapped(
        MockLanguageModel::builder()
            .generate(text_result(
                "<think>analyzing the request</think>Here is the response",
            ))
            .build_shared(),
        extract_reasoning("think"),
    );
    let result = generate(&model).await;
    assert_eq!(
        render_content(&result.content),
        vec![
            "reasoning:analyzing the request",
            "text:Here is the response"
        ]
    );
}

#[tokio::test]
async fn should_extract_reasoning_from_think_tags_when_there_is_no_text() {
    let model = wrapped(
        MockLanguageModel::builder()
            .generate(text_result("<think>analyzing the request</think>"))
            .build_shared(),
        extract_reasoning("think"),
    );
    let result = generate(&model).await;
    assert_eq!(
        render_content(&result.content),
        vec!["reasoning:analyzing the request", "text:"]
    );
}

#[tokio::test]
async fn should_extract_reasoning_from_multiple_think_tags() {
    let model = wrapped(
        MockLanguageModel::builder()
            .generate(text_result(
                "<think>analyzing the request</think>Here is the response<think>thinking about the response</think>more",
            ))
            .build_shared(),
        extract_reasoning("think"),
    );
    let result = generate(&model).await;
    assert_eq!(
        render_content(&result.content),
        vec![
            "reasoning:analyzing the request\nthinking about the response",
            "text:Here is the response\nmore"
        ]
    );
}

#[tokio::test]
async fn should_prepend_think_tag_iff_start_with_reasoning_is_true() {
    let text = "analyzing the request</think>Here is the response";
    let model = wrapped(
        MockLanguageModel::builder()
            .generate(text_result(text))
            .build_shared(),
        extract_reasoning("think").start_with_reasoning(true),
    );
    assert_eq!(
        render_content(&generate(&model).await.content),
        vec![
            "reasoning:analyzing the request",
            "text:Here is the response"
        ]
    );

    let model = wrapped(
        MockLanguageModel::builder()
            .generate(text_result(text))
            .build_shared(),
        extract_reasoning("think"),
    );
    assert_eq!(
        render_content(&generate(&model).await.content),
        vec![format!("text:{text}")]
    );
}

#[tokio::test]
async fn should_preserve_reasoning_property_even_when_rest_contains_other_properties() {
    let mut result = text_result("<think>analyzing the request</think>Here is the response");
    result.usage = Usage::totals(3, 7);
    result.warnings = vec![Warning::other("careful")];
    result.provider_metadata = Some(
        [(
            "mock".to_owned(),
            json!({ "a": 1 }).as_object().unwrap().clone(),
        )]
        .into_iter()
        .collect(),
    );
    result.response.id = Some("resp-1".to_owned());
    let model = wrapped(
        MockLanguageModel::builder().generate(result).build_shared(),
        extract_reasoning("think"),
    );
    let result: GenerateResult = generate(&model).await;
    assert_eq!(
        render_content(&result.content),
        vec![
            "reasoning:analyzing the request",
            "text:Here is the response"
        ]
    );
    assert_eq!(result.usage, Usage::totals(3, 7));
    assert_eq!(result.warnings, vec![Warning::other("careful")]);
    assert!(result.provider_metadata.is_some());
    assert_eq!(result.response.id.as_deref(), Some("resp-1"));
    assert_eq!(result.finish_reason, FinishReason::stop());
}

// ---- wrapStream ----

#[tokio::test]
async fn should_not_read_object_prototype_for_missing_text_part_ids() {
    let parts = vec![
        StreamPart::stream_start(),
        StreamPart::text_delta(PartId::new("constructor"), "hello"),
        StreamPart::text_delta(PartId::new("toString"), " world"),
        StreamPart::finish(FinishReason::stop(), Usage::totals(1, 1)),
    ];
    let model = wrapped(
        MockLanguageModel::builder().stream(parts).build_shared(),
        extract_reasoning("think"),
    );
    assert_eq!(
        render(&stream(&model).await),
        vec![
            "stream-start",
            "text-delta(constructor):hello",
            "text-delta(toString): world",
            "finish",
        ]
    );
}

#[tokio::test]
async fn should_extract_reasoning_from_split_think_tags() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream(
                "1",
                &["<thi", "nk>ana", "lysis</think>", " here is the result"],
            ))
            .build_shared(),
        extract_reasoning("think"),
    );
    assert_eq!(
        render(&stream(&model).await),
        vec![
            "stream-start",
            "reasoning-start(reasoning-0)",
            "reasoning-delta(reasoning-0):ana",
            "reasoning-delta(reasoning-0):lysis",
            "reasoning-end(reasoning-0)",
            "text-start(1)",
            "text-delta(1): here is the result",
            "text-end(1)",
            "finish",
        ]
    );
}

#[tokio::test]
async fn should_extract_reasoning_from_single_chunk_with_multiple_think_tags() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream(
                "1",
                &["<think>ana</think>text<think>lysis</think>more"],
            ))
            .build_shared(),
        extract_reasoning("think"),
    );
    assert_eq!(
        render(&stream(&model).await),
        vec![
            "stream-start",
            "reasoning-start(reasoning-0)",
            "reasoning-delta(reasoning-0):ana",
            "reasoning-end(reasoning-0)",
            "text-start(1)",
            "text-delta(1):text",
            "reasoning-start(reasoning-1)",
            "reasoning-delta(reasoning-1):\nlysis",
            "reasoning-end(reasoning-1)",
            "text-delta(1):\nmore",
            "text-end(1)",
            "finish",
        ]
    );
}

#[tokio::test]
async fn should_extract_reasoning_from_think_when_there_is_no_text() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream("1", &["<think>ana", "lysis</think>"]))
            .build_shared(),
        extract_reasoning("think"),
    );
    assert_eq!(
        render(&stream(&model).await),
        vec![
            "stream-start",
            "reasoning-start(reasoning-0)",
            "reasoning-delta(reasoning-0):ana",
            "reasoning-delta(reasoning-0):lysis",
            "reasoning-end(reasoning-0)",
            "text-start(1)",
            "text-end(1)",
            "finish",
        ]
    );
}

#[tokio::test]
async fn should_prepend_think_tag_if_start_with_reasoning_is_true() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream("1", &["ana", "lysis</think>", "text"]))
            .build_shared(),
        extract_reasoning("think").start_with_reasoning(true),
    );
    assert_eq!(
        render(&stream(&model).await),
        vec![
            "stream-start",
            "reasoning-start(reasoning-0)",
            "reasoning-delta(reasoning-0):ana",
            "reasoning-delta(reasoning-0):lysis",
            "reasoning-end(reasoning-0)",
            "text-start(1)",
            "text-delta(1):text",
            "text-end(1)",
            "finish",
        ]
    );
}

#[tokio::test]
async fn should_keep_original_text_when_think_tag_is_not_present() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream("1", &["hello ", "world"]))
            .build_shared(),
        extract_reasoning("think"),
    );
    assert_eq!(
        render(&stream(&model).await),
        vec![
            "stream-start",
            "text-start(1)",
            "text-delta(1):hello ",
            "text-delta(1):world",
            "text-end(1)",
            "finish",
        ]
    );
}

#[tokio::test]
async fn should_handle_empty_think_tags_without_crashing() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream("1", &["<think></think>text"]))
            .build_shared(),
        extract_reasoning("think"),
    );
    assert_eq!(
        render(&stream(&model).await),
        vec![
            "stream-start",
            "reasoning-start(reasoning-0)",
            "reasoning-end(reasoning-0)",
            "text-start(1)",
            "text-delta(1):text",
            "text-end(1)",
            "finish",
        ]
    );
}

#[tokio::test]
async fn custom_separator_and_partial_tag_at_stream_end() {
    let model = wrapped(
        MockLanguageModel::builder()
            .stream(text_stream(
                "1",
                &["<think>a</think>b<think>c</think>d", "<thi"],
            ))
            .build_shared(),
        extract_reasoning("think").separator(" | "),
    );
    assert_eq!(
        render(&stream(&model).await),
        vec![
            "stream-start",
            "reasoning-start(reasoning-0)",
            "reasoning-delta(reasoning-0):a",
            "reasoning-end(reasoning-0)",
            "text-start(1)",
            "text-delta(1):b",
            "reasoning-start(reasoning-1)",
            "reasoning-delta(reasoning-1): | c",
            "reasoning-end(reasoning-1)",
            "text-delta(1): | d",
            "text-end(1)",
            "finish",
        ]
    );
}
