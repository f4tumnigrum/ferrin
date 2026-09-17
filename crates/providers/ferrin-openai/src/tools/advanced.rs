//! Advanced tool schemas and provider caller binding.
//!
//! Protocol behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolCallerDefinition;
use serde_json::json;

pub(super) fn programmatic() -> Tool {
    Tool::provider_executed("openai.programmatic_tool_calling", JsonObject::new())
        .input_schema(Schema::from_json_schema(json!({
            "type": "object", "properties": {"code": {"type": "string"}, "fingerprint": {"type": "string"}},
            "required": ["code", "fingerprint"]
        })))
        .output_schema(Schema::from_json_schema(json!({
            "type": "object", "properties": {"result": {"type": "string"}, "status": {"enum": ["completed", "incomplete"]}},
            "required": ["result", "status"]
        })))
        .supports_deferred_results(true)
        .caller(ToolCallerDefinition::provider(|options| {
            let mut options = options.unwrap_or_default();
            let openai = options.entry("openai".to_owned()).or_default();
            let mut callers = openai.get("allowedCallers").and_then(JsonValue::as_array).cloned().unwrap_or_default();
            let caller = JsonValue::from("programmatic");
            if !callers.contains(&caller) { callers.push(caller); }
            openai.insert("allowedCallers".into(), JsonValue::Array(callers));
            options
        }))
        .build()
}

pub(super) fn search(arguments: JsonObject) -> Tool {
    let builder = if arguments.get("execution").and_then(JsonValue::as_str) == Some("client") {
        Tool::provider_defined("openai.tool_search", arguments)
    } else {
        Tool::provider_executed("openai.tool_search", arguments)
    };
    builder.input_schema(Schema::from_json_schema(json!({
        "type": "object", "properties": {"arguments": {}, "call_id": {"type": ["string", "null"]}}
    }))).output_schema(Schema::from_json_schema(json!({
        "type": "object", "properties": {"tools": {"type": "array", "items": {"type": "object"}}}, "required": ["tools"]
    }))).build()
}

pub(super) fn shell(arguments: JsonObject) -> Tool {
    let hosted = arguments
        .get("environment")
        .and_then(|v| v.get("type"))
        .and_then(JsonValue::as_str)
        .is_some_and(|kind| kind != "local");
    let builder = if hosted {
        Tool::provider_executed("openai.shell", arguments)
    } else {
        Tool::provider_defined("openai.shell", arguments)
    };
    builder.input_schema(Schema::from_json_schema(json!({
        "type": "object", "properties": {"action": {"type": "object", "properties": {
            "commands": {"type": "array", "items": {"type": "string"}}, "timeoutMs": {"type": ["integer", "null"]}, "maxOutputLength": {"type": ["integer", "null"]}
        }, "required": ["commands"]}}, "required": ["action"]
    }))).output_schema(Schema::from_json_schema(json!({
        "type":"object", "properties":{"output":{"type":"array","items":{
            "type":"object", "properties":{
                "stdout":{"type":"string"}, "stderr":{"type":"string"},
                "outcome":{"oneOf":[
                    {"type":"object","properties":{"type":{"const":"exit"},"exitCode":{"type":"integer"}},"required":["type","exitCode"]},
                    {"type":"object","properties":{"type":{"const":"timeout"}},"required":["type"]}
                ]}
            }, "required":["stdout","stderr","outcome"]
        }}}, "required":["output"]
    }))).build()
}
