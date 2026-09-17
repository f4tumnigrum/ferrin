//! Tool definition metadata survives parsing, execution, streaming and replay.

use std::sync::Arc;

use ferrin_core::StepContent;
use ferrin_core::StreamEvent;
use ferrin_core::generate_text;
use ferrin_core::generate_text::RepairRequest;
use ferrin_core::generate_text::ToolCallRepair;
use ferrin_core::stream_text;
use ferrin_message::Message;
use ferrin_message::ToolApprovalResponse;
use ferrin_spec::BoxFuture;
use ferrin_spec::Content;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::Usage;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::ProviderToolResult;
use ferrin_tool::NeedsApproval;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::tool_call_result;

fn metadata() -> JsonObject {
    json!({
        "clientName": "catalog",
        "toolName": "lookup",
        "annotations": { "readOnlyHint": true }
    })
    .as_object()
    .unwrap()
    .clone()
}

fn normalize(content: Vec<StepContent>) -> JsonValue {
    let mut serialized = serde_json::to_value(content).unwrap();
    for part in serialized.as_array_mut().unwrap() {
        part.as_object_mut().unwrap().remove("execution_ms");
    }
    serialized
}

async fn run_pair(content: Vec<Content>, tools: ToolSet) -> [JsonValue; 2] {
    let mut parts = vec![StreamPart::stream_start()];
    parts.extend(content.iter().map(|part| match part {
        Content::ToolCall(call) => StreamPart::ToolCall(call.clone()),
        Content::ToolResult(result) => StreamPart::ToolResult(result.clone()),
        other => panic!("unexpected test content: {other:?}"),
    }));
    parts.push(StreamPart::finish(
        FinishReason::tool_calls(),
        Usage::default(),
    ));
    let model = mock()
        .generate(GenerateResult::new(content, FinishReason::tool_calls()))
        .stream(parts)
        .build_shared();
    let generated = generate_text(Arc::clone(&model))
        .prompt("lookup")
        .tools(tools.clone())
        .await
        .unwrap();
    let streamed = stream_text(model)
        .prompt("lookup")
        .tools(tools)
        .await
        .unwrap()
        .consume()
        .await
        .unwrap();
    [generated, streamed].map(|result| normalize(result.last_step().content.clone()))
}

#[tokio::test]
async fn tool_metadata_survives_function_and_dynamic_execution() {
    for kind in ["function", "dynamic"] {
        let builder = if kind == "dynamic" {
            Tool::dynamic(Schema::empty_object())
        } else {
            Tool::function_with_schema(Schema::empty_object())
        };
        let tool = builder
            .metadata(metadata())
            .execute(|_, _| async { Ok::<_, ToolError>(json!({ "found": true })) })
            .build();
        let results = run_pair(
            vec![Content::ToolCall(ToolCall::new("call", "lookup", "{}"))],
            ToolSet::new().insert("lookup", tool).unwrap(),
        )
        .await;
        let mut expected = json!([
            { "type": "tool-call", "tool_call_id": "call", "tool_name": "lookup",
              "input": {}, "tool_metadata": metadata() },
            { "type": "tool-result", "tool_call_id": "call", "tool_name": "lookup",
              "input": {}, "output": { "found": true }, "tool_metadata": metadata() }
        ]);
        if kind == "dynamic" {
            for part in expected.as_array_mut().unwrap() {
                part["dynamic"] = json!(true);
            }
        }
        assert_eq!(results, [expected.clone(), expected]);
    }
}

#[tokio::test]
async fn tool_metadata_survives_execution_errors_and_invalid_inputs() {
    let tools = ToolSet::new()
        .insert(
            "lookup",
            Tool::function_with_schema(Schema::empty_object())
                .metadata(metadata())
                .execute(|_, _| async {
                    Err::<JsonValue, _>(ToolError::json(json!({ "message": "unavailable" })))
                })
                .build(),
        )
        .unwrap();
    let results = run_pair(
        vec![Content::ToolCall(ToolCall::new("call", "lookup", "{}"))],
        tools.clone(),
    )
    .await;
    let expected = json!([
        { "type": "tool-call", "tool_call_id": "call", "tool_name": "lookup",
          "input": {}, "tool_metadata": metadata() },
        { "type": "tool-error", "tool_call_id": "call", "tool_name": "lookup",
          "input": {}, "error": { "type": "json", "value": { "message": "unavailable" } },
          "tool_metadata": metadata() }
    ]);
    assert_eq!(results, [expected.clone(), expected]);
    for name in ["lookup", "unknown"] {
        let results = run_pair(
            vec![Content::ToolCall(ToolCall::new("call", name, "{"))],
            tools.clone(),
        )
        .await;
        assert_eq!(results[0], results[1]);
        let expected_metadata = if name == "lookup" {
            json!(metadata())
        } else {
            JsonValue::Null
        };
        assert_eq!(
            (
                results[0][0]["invalid"].clone(),
                results[0][0]["tool_metadata"].clone(),
                results[0][1]["tool_metadata"].clone(),
            ),
            (json!(true), expected_metadata.clone(), expected_metadata)
        );
    }
}

struct RepairToLookup;

impl ToolCallRepair for RepairToLookup {
    fn repair<'a>(
        &'a self,
        request: RepairRequest<'a>,
    ) -> BoxFuture<'a, Result<Option<ToolCall>, ferrin_core::error::BoxError>> {
        Box::pin(async move {
            Ok(Some(ToolCall::new(
                request.tool_call.tool_call_id.clone(),
                "lookup",
                "{}",
            )))
        })
    }
}

#[tokio::test]
async fn tool_metadata_comes_from_the_repaired_tool() {
    let tools = ToolSet::new()
        .insert(
            "lookup",
            Tool::function_with_schema(Schema::empty_object())
                .metadata(metadata())
                .build(),
        )
        .unwrap();
    let call = ToolCall::new("call", "unknown", "{}");
    let model = mock()
        .generate(tool_call_result("call", "unknown", &json!({})))
        .stream(vec![
            StreamPart::stream_start(),
            StreamPart::ToolCall(call),
            StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
        ])
        .build_shared();
    let generated = generate_text(Arc::clone(&model))
        .prompt("lookup")
        .tools(tools.clone())
        .repair_tool_call(RepairToLookup)
        .await
        .unwrap();
    let streamed = stream_text(model)
        .prompt("lookup")
        .tools(tools)
        .repair_tool_call(RepairToLookup)
        .await
        .unwrap()
        .consume()
        .await
        .unwrap();
    let expected = json!([
        { "type": "tool-call", "tool_call_id": "call", "tool_name": "lookup",
          "input": {}, "tool_metadata": metadata() }
    ]);
    for result in [generated, streamed] {
        assert_eq!(normalize(result.last_step().content.clone()), expected);
    }
}

#[tokio::test]
async fn tool_metadata_survives_provider_results_and_deferred_errors() {
    for outcome in ["success", "error", "deferred"] {
        let tool = Tool::provider_executed("mock.lookup", JsonObject::new())
            .metadata(metadata())
            .build();
        let mut call = ToolCall::new("call", "lookup", "{}");
        call.provider_executed = true;
        let result = ProviderToolResult {
            tool_call_id: "call".into(),
            tool_name: "lookup".into(),
            result: json!({ "found": true }),
            is_error: outcome != "success",
            preliminary: false,
            dynamic: false,
            provider_metadata: Some(
                serde_json::from_value(json!({ "mock": { "id": "trace" } })).unwrap(),
            ),
        };
        let mut content = Vec::new();
        if outcome != "deferred" {
            content.push(Content::ToolCall(call));
        }
        content.push(Content::ToolResult(result));
        let results = run_pair(content, ToolSet::new().insert("lookup", tool).unwrap()).await;
        let mut expected = json!({
            "type": "tool-result", "tool_call_id": "call", "tool_name": "lookup",
            "input": {}, "output": { "found": true }, "provider_executed": true,
            "tool_metadata": metadata(), "provider_metadata": { "mock": { "id": "trace" } }
        });
        if outcome != "success" {
            expected["type"] = json!("tool-error");
            expected.as_object_mut().unwrap().remove("output");
            expected["error"] = json!({ "type": "json", "value": { "found": true } });
        }
        if outcome == "deferred" {
            expected["input"] = JsonValue::Null;
        }
        for result in results {
            assert_eq!(result.as_array().unwrap().last().unwrap(), &expected);
        }
    }
}

#[tokio::test]
async fn tool_metadata_survives_preliminary_stream_results() {
    let tool = Tool::dynamic(Schema::empty_object())
        .metadata(metadata())
        .execute_stream(|_, _| futures_util::stream::iter([Ok::<_, ToolError>(1), Ok(2)]))
        .build();
    let model = mock()
        .stream(vec![
            StreamPart::stream_start(),
            StreamPart::ToolCall(ToolCall::new("call", "lookup", "{}")),
            StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
        ])
        .build_shared();
    let (events, completion) = stream_text(model)
        .prompt("lookup")
        .tools(ToolSet::new().insert("lookup", tool).unwrap())
        .await
        .unwrap()
        .split();
    let results: Vec<_> = events
        .filter_map(|event| async move {
            match event {
                StreamEvent::ToolResult(result) => {
                    Some((result.output, result.preliminary, result.tool_metadata))
                }
                _ => None,
            }
        })
        .collect()
        .await;
    completion.await.unwrap();
    assert_eq!(
        results,
        vec![
            (json!(1), true, Some(metadata())),
            (json!(2), true, Some(metadata())),
            (json!(2), false, Some(metadata())),
        ]
    );
}

#[tokio::test]
async fn tool_metadata_is_restored_from_current_definitions_during_replay() {
    for decision in ["approved", "denied"] {
        let make_tools = |metadata| {
            ToolSet::new()
                .insert(
                    "lookup",
                    Tool::dynamic(Schema::empty_object())
                        .metadata(metadata)
                        .needs_approval(NeedsApproval::Always)
                        .execute(|_, _| async { Ok::<_, ToolError>(json!({ "found": true })) })
                        .build(),
                )
                .unwrap()
        };
        let first = generate_text(
            mock()
                .generate(tool_call_result("call", "lookup", &json!({})))
                .build_shared(),
        )
        .prompt("lookup")
        .tools(make_tools(JsonObject::new()))
        .await
        .unwrap();
        let approval_id = first
            .last_step()
            .tool_approval_requests()
            .next()
            .unwrap()
            .approval_id
            .clone();
        let mut history = vec![Message::user("lookup")];
        history.extend(first.response_messages());
        let response = if decision == "approved" {
            ToolApprovalResponse::approved(approval_id)
        } else {
            ToolApprovalResponse::denied(approval_id)
        };
        history.push(Message::tool([response]));
        let model = mock()
            .stream(ferrin_testing::text_parts(["done"], Usage::default()))
            .build_shared();
        let (events, completion) = stream_text(model)
            .messages(history)
            .tools(make_tools(metadata()))
            .await
            .unwrap()
            .split();
        let actual: Vec<_> = events
            .filter_map(|event| async move {
                match event {
                    StreamEvent::ToolResult(result) => Some(("approved", result.tool_metadata)),
                    StreamEvent::ToolOutputDenied(result) => Some(("denied", result.tool_metadata)),
                    _ => None,
                }
            })
            .collect()
            .await;
        completion.await.unwrap();
        assert_eq!(actual, vec![(decision, Some(metadata()))]);
    }
}

#[test]
fn tool_metadata_fields_preserve_historical_serialization() {
    for kind in [
        "tool-call",
        "tool-result",
        "tool-error",
        "tool-output-denied",
    ] {
        let mut value = json!({
            "type": kind, "tool_call_id": "call", "tool_name": "lookup", "input": {}
        });
        if kind == "tool-result" {
            value["output"] = json!({});
        } else if kind == "tool-error" {
            value["error"] = json!({ "type": "text", "message": "failed" });
        }
        let decoded: StepContent = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
        value["tool_metadata"] = json!(metadata());
        let decoded: StepContent = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
        value["provider_metadata"] = json!({
            "openai": { "parallelToolCall": { "id": "wrapper", "index": 0 } }
        });
        let decoded: StepContent = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
    }
}
