//! Real Responses API exchanges recorded through a third-party proxy.

use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::GenerateResult;
use ferrin_spec::JsonValue;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::ResponseFormat;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::fixture_bytes;
use super::common::openai_options;
use super::common::without_raw_usage;

const AREA: &str = "responses/recorded-proxy";
const MODEL: &str = "gpt-5.6-sol";

fn fixture_json(case: &str, suffix: &str) -> JsonValue {
    serde_json::from_slice(&fixture_bytes(AREA, &format!("{case}.{suffix}.json"))).unwrap()
}

fn options(case: &str) -> CallOptions {
    let request = fixture_json(case, "request");
    let prompt = request["input"][0]["content"][0]["text"].as_str().unwrap();
    let mut options = CallOptions::new(vec![PromptMessage::user_text(prompt)]);
    options.max_output_tokens = Some(512);
    options.provider_options = openai_options(json!({"store": false, "reasoningEffort": "low"}));
    if case == "tool-call" {
        options.tools = vec![ToolDefinition::function(
            "get_weather",
            Some("Returns the current weather for a city.".to_owned()),
            json!({"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"], "additionalProperties": false}),
        )];
        if let ToolDefinition::Function { strict, .. } = &mut options.tools[0] {
            *strict = Some(true);
        }
        options.tool_choice = Some(ToolChoice::tool("get_weather"));
    }
    if case == "structured-output" {
        options.response_format = Some(ResponseFormat::Json {
            schema: Some(
                json!({"type": "object", "properties": {"city": {"type": "string"}, "country": {"type": "string"}}, "required": ["city", "country"], "additionalProperties": false}),
            ),
            name: Some("capital".to_owned()),
            description: None,
        });
    }
    options
}

async fn generate(case: &str) -> GenerateResult {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/responses", AREA, case);
    let result = test
        .provider
        .responses(MODEL)
        .do_generate(options(case))
        .await
        .unwrap();
    assert_eq!(
        test.only_request().body_json().unwrap(),
        fixture_json(case, "request")
    );
    let raw = fixture_json(case, "response");
    assert_eq!(
        json!({"id": result.response.id, "model": result.response.model_id, "usage": result.usage.raw}),
        json!({"id": raw["id"], "model": raw["model"], "usage": raw["usage"]})
    );
    assert!(result.warnings.is_empty());
    let mut usage = result.usage.clone();
    usage.raw = None;
    let calls: Vec<_> = result.content.iter().filter_map(Content::as_tool_call).map(|call| json!({"name": call.tool_name, "input": serde_json::from_str::<JsonValue>(&call.input).unwrap(), "provider_executed": call.provider_executed})).collect();
    insta::assert_json_snapshot!(
        format!("recorded-proxy_{case}"),
        json!({
            "text": result.content.iter().filter_map(Content::as_text).collect::<String>(),
            "calls": calls,
            "finish_reason": result.finish_reason,
            "usage": usage,
            "warnings": result.warnings,
        })
    );
    result
}

#[tokio::test]
async fn recorded_text_maps_content_and_full_usage() {
    let result = generate("text-basic").await;
    assert_eq!(
        result
            .content
            .iter()
            .filter_map(Content::as_text)
            .collect::<String>(),
        "pong"
    );
}

#[tokio::test]
async fn recorded_tool_call_maps_name_and_arguments() {
    let result = generate("tool-call").await;
    let calls: Vec<_> = result.content.iter().filter_map(Content::as_tool_call).map(|call| json!({"name": call.tool_name, "input": serde_json::from_str::<JsonValue>(&call.input).unwrap()})).collect();
    assert_eq!(
        calls,
        vec![json!({"name": "get_weather", "input": {"city": "Berlin"}})]
    );
}

#[tokio::test]
async fn recorded_structured_output_is_the_requested_object() {
    let result = generate("structured-output").await;
    let text = result
        .content
        .iter()
        .filter_map(Content::as_text)
        .collect::<String>();
    assert_eq!(
        serde_json::from_str::<JsonValue>(&text).unwrap(),
        json!({"city": "Paris", "country": "France"})
    );
}

#[tokio::test]
async fn recorded_stream_obeys_contract_and_preserves_usage() {
    let case = "text-basic-stream";
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/responses", AREA, case);
    let result = test
        .provider
        .responses(MODEL)
        .do_stream(options(case))
        .await
        .unwrap();
    let parts = collect_checked(result).await;
    assert_eq!(
        test.only_request().body_json().unwrap(),
        fixture_json(case, "request")
    );
    let text = parts
        .iter()
        .filter_map(|part| match part {
            StreamPart::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, "pong");
    assert!(
        !parts
            .iter()
            .any(|part| matches!(part, StreamPart::Error { .. }))
    );
    let Some(StreamPart::Finish {
        finish_reason,
        usage,
        ..
    }) = parts.last()
    else {
        panic!("expected a terminal Finish event");
    };
    let events = ferrin_testing::fixture::decode_events_file(
        &String::from_utf8(fixture_bytes(AREA, &format!("{case}.chunks.txt"))).unwrap(),
    );
    let payload = ferrin_provider_util::sse::SseDecoder::new()
        .feed(format!("{}\n\n", events.last().unwrap()).as_bytes())
        .unwrap();
    let raw: JsonValue = serde_json::from_str(&payload[0].data).unwrap();
    assert_eq!(json!(usage.raw), raw["response"]["usage"]);
    let mut normalized_usage = usage.clone();
    normalized_usage.raw = None;
    insta::assert_json_snapshot!(
        "recorded-proxy_text_stream",
        json!({"text": text, "finish_reason": finish_reason, "usage": normalized_usage})
    );
    insta::assert_json_snapshot!("recorded-proxy_stream_events", without_raw_usage(parts), {
        "[].id" => "[id]",
        "[].timestamp" => "[timestamp]",
        "[].provider_metadata.openai.itemId" => "[item-id]",
        "[].provider_metadata.openai.responseId" => "[response-id]",
    });
}
