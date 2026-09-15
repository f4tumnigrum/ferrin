//! Streaming `generateContent` calls replayed from fixtures.

use ferrin_spec::CallOptions;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolDefinition;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::without_raw_usage;

fn path(model: &str) -> String {
    format!("/v1beta/models/{model}:streamGenerateContent")
}

fn options() -> CallOptions {
    CallOptions::new(vec![PromptMessage::user_text(
        "How many r's are in strawberry?",
    )])
}

fn finish(parts: &[StreamPart]) -> (&ferrin_spec::FinishReason, &ferrin_spec::Usage) {
    let Some(StreamPart::Finish {
        finish_reason,
        usage,
        ..
    }) = parts.last()
    else {
        panic!("expected finish, got {:?}", parts.last());
    };
    (finish_reason, usage)
}

fn text(parts: &[StreamPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

fn tool_calls(parts: &[StreamPart]) -> Vec<(&str, &str)> {
    parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::ToolCall(call) => Some((call.tool_name.as_str(), call.input.as_str())),
            _ => None,
        })
        .collect()
}

async fn stream(test: &TestProvider, model: &str, options: CallOptions) -> Vec<StreamPart> {
    let result = test
        .provider
        .language_model(model)
        .do_stream(options)
        .await
        .unwrap();
    without_raw_usage(collect_checked(result).await)
}

#[tokio::test]
async fn text_stream_emits_text_parts_and_finish() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-3-pro-preview"),
        "stream",
        "text",
    );
    let parts = stream(&test, "gemini-3-pro-preview", options()).await;
    assert_eq!(
        text(&parts),
        "There are **3** \"r\"s in strawberry.\n\nst**r**awbe**rr**y"
    );
    assert!(matches!(
        &parts[1],
        StreamPart::ResponseMetadata { id: Some(id), .. } if id == "bH6LaZW8Fp_3nsEPqtaSwQ4"
    ));
    let (reason, usage) = finish(&parts);
    assert_eq!(reason.unified, FinishReasonKind::Stop);
    assert_eq!(usage.input.total, Some(9));
    assert_eq!(usage.output.text, Some(23));
    assert_eq!(usage.output.reasoning, Some(185));
    insta::assert_json_snapshot!("stream_text", parts);
    let request = test.only_request();
    assert_eq!(request.path, path("gemini-3-pro-preview"));
    assert_eq!(request.query.as_deref(), Some("alt=sse"));
}

#[tokio::test]
async fn complete_function_calls_emit_input_parts_then_a_tool_call() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-3-pro-preview"),
        "stream",
        "tool-call",
    );
    let mut options = options();
    options.tools = vec![ToolDefinition::function(
        "weather",
        None,
        json!({"type": "object", "properties": {"location": {"type": "string"}}}),
    )];
    let parts = stream(&test, "gemini-3-pro-preview", options).await;
    assert_eq!(
        tool_calls(&parts),
        vec![("weather", "{\"location\":\"San Francisco\"}")]
    );
    let (reason, _) = finish(&parts);
    assert_eq!(reason.unified, FinishReasonKind::ToolCalls);
    insta::assert_json_snapshot!("stream_tool_call", parts);
}

#[tokio::test]
async fn streamed_function_call_arguments_are_accumulated() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-3.1-pro-preview"),
        "stream",
        "tool-call-arguments",
    );
    let mut options = options();
    options.tools = vec![ToolDefinition::function(
        "getWeather",
        None,
        json!({"type": "object", "properties": {"location": {"type": "string"}}}),
    )];
    let parts = stream(&test, "gemini-3.1-pro-preview", options).await;
    assert_eq!(
        tool_calls(&parts),
        vec![
            ("getWeather", "{\"location\":\"Boston\"}"),
            ("getWeather", "{\"location\":\"San Francisco\"}"),
        ]
    );
    let deltas: Vec<&str> = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::ToolInputDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        deltas,
        vec![
            "{\"location\":\"Boston",
            "\"}",
            "{\"location\":\"San Francisco",
            "\"}",
        ]
    );
    insta::assert_json_snapshot!("stream_tool_call_arguments", parts);
}

#[tokio::test]
async fn no_argument_calls_and_repeated_streamed_calls_are_separated() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-3-flash-preview"),
        "stream",
        "no-args-tool-call",
    );
    let parts = stream(&test, "gemini-3-flash-preview", options()).await;
    assert_eq!(
        tool_calls(&parts),
        vec![
            ("read_theme", "{}"),
            ("read_screen", "{\"id\":\"A\"}"),
            ("read_screen", "{\"id\":\"B\"}"),
            ("read_screen", "{\"id\":\"C\"}"),
        ]
    );
    assert!(
        parts
            .iter()
            .any(|part| matches!(part, StreamPart::ReasoningDelta { .. }))
    );
    let (reason, usage) = finish(&parts);
    assert_eq!(reason.unified, FinishReasonKind::ToolCalls);
    assert_eq!(usage.output.reasoning, Some(183));
    insta::assert_json_snapshot!("stream_no_args_tool_call", parts);
}

#[tokio::test]
async fn reasoning_and_text_blocks_carry_signatures() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-2.5-flash"),
        "stream",
        "reasoning",
    );
    let parts = stream(&test, "gemini-2.5-flash", options()).await;
    insta::assert_json_snapshot!("stream_reasoning", parts);
    assert_eq!(text(&parts), "There are 3 r's.");
    let (reason, usage) = finish(&parts);
    assert_eq!(reason.unified, FinishReasonKind::Stop);
    assert_eq!(usage.output.reasoning, Some(40));
}

#[tokio::test]
async fn code_execution_and_sources_are_streamed_once() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-2.5-flash"),
        "stream",
        "code-execution",
    );
    let parts = stream(&test, "gemini-2.5-flash", options()).await;
    let sources = parts
        .iter()
        .filter(|part| matches!(part, StreamPart::Source(_)))
        .count();
    assert_eq!(sources, 1);
    assert!(
        parts
            .iter()
            .any(|part| matches!(part, StreamPart::ToolResult(_)))
    );
    insta::assert_json_snapshot!("stream_code_execution", parts);
}

#[tokio::test]
async fn blocked_prompts_finish_with_content_filter() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, &path("gemini-2.5-flash"), "stream", "blocked");
    let parts = stream(&test, "gemini-2.5-flash", options()).await;
    let (reason, _) = finish(&parts);
    assert_eq!(reason.unified, FinishReasonKind::ContentFilter);
    assert_eq!(reason.raw.as_deref(), Some("SAFETY"));
    assert!(text(&parts).is_empty());
}

#[tokio::test]
async fn inline_data_is_emitted_as_a_file_part_after_closing_text() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-2.5-flash-image"),
        "stream",
        "inline-image",
    );
    let parts = stream(&test, "gemini-2.5-flash-image", options()).await;
    let file_index = parts
        .iter()
        .position(|part| matches!(part, StreamPart::File { .. }))
        .unwrap();
    let text_end = parts
        .iter()
        .position(|part| matches!(part, StreamPart::TextEnd { .. }))
        .unwrap();
    assert!(text_end < file_index);
    insta::assert_json_snapshot!("stream_inline_image", parts);
}

#[tokio::test]
async fn raw_chunks_are_forwarded_when_requested() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        &path("gemini-3-pro-preview"),
        "stream",
        "text",
    );
    let mut options = options();
    options.include_raw_chunks = true;
    let parts = stream(&test, "gemini-3-pro-preview", options).await;
    let raw = parts
        .iter()
        .filter(|part| matches!(part, StreamPart::Raw { .. }))
        .count();
    assert_eq!(raw, 3);
}

#[tokio::test]
async fn preterminal_fixture_boundaries_report_truncation() {
    for fixture in ["text", "reasoning", "tool-call-arguments"] {
        let raw = String::from_utf8(super::common::fixture_bytes(
            "stream",
            &format!("{fixture}.chunks.txt"),
        ))
        .unwrap();
        let events: Vec<_> = raw.lines().filter(|line| !line.is_empty()).collect();
        let terminal = events
            .iter()
            .position(|line| line.contains("finishReason"))
            .unwrap();
        for end in 0..=terminal {
            let test = TestProvider::start().await;
            test.mount_fixture(
                Method::POST,
                &path("gemini-3-pro-preview"),
                ferrin_testing::Fixture::sse(events[..end].iter().copied()),
            );
            let parts = stream(&test, "gemini-3-pro-preview", options()).await;
            let terminal_parts: Vec<_> = parts
                .iter()
                .filter_map(|part| match part {
                    StreamPart::Error { .. } => Some("error"),
                    StreamPart::Finish { .. } => Some("finish"),
                    _ => None,
                })
                .collect();
            assert_eq!(
                terminal_parts,
                vec!["error"],
                "{fixture} truncated at {end}"
            );
            if fixture == "tool-call-arguments" && end < 4 {
                assert!(
                    tool_calls(&parts).is_empty(),
                    "unfinished tool emitted at {end}"
                );
            }
        }
    }
}
