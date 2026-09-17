//! Code-execution input/output validation and deferred provider caller binding.
//!
//! Schemas and caller behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolCallerDefinition;
use serde_json::json;

fn variant(kind: &str, mut properties: JsonValue, fields: &[&str]) -> JsonValue {
    properties["type"] = json!({"const":kind});
    let required: Vec<_> = std::iter::once("type")
        .chain(fields.iter().copied())
        .collect();
    json!({"type":"object","properties":properties,"required":required})
}

fn input_schema() -> Schema<JsonValue> {
    let editor = |command: &str, fields: JsonValue, required: &[&str]| {
        let mut properties = fields;
        properties["command"] = json!({"const":command});
        properties["path"] = json!({"type":"string"});
        let required: Vec<_> = ["command", "path"]
            .into_iter()
            .chain(required.iter().copied())
            .collect();
        variant("text_editor_code_execution", properties, &required)
    };
    Schema::from_json_schema(json!({"oneOf":[
        variant("programmatic-tool-call",json!({"code":{"type":"string"}}), &["code"]),
        variant("bash_code_execution",json!({"command":{"type":"string"}}), &["command"]),
        editor("view",json!({}),&[]),
        editor("create",json!({"file_text":{"type":["string","null"]}}),&[]),
        editor("str_replace",json!({"old_str":{"type":"string"},"new_str":{"type":"string"}}),&["old_str","new_str"])
    ]}))
}

fn output_schema(kind: &str) -> Schema<JsonValue> {
    let execution = |kind: &str, stdout: &str, content_type: &str| {
        variant(
            kind,
            json!({
                stdout:{"type":"string"},"stderr":{"type":"string"},"return_code":{"type":"number"},
                "content":{"type":"array","items":variant(content_type,json!({"file_id":{"type":"string"}}), &["file_id"])}
            }),
            &[stdout, "stderr", "return_code"],
        )
    };
    let mut variants = vec![
        execution("code_execution_result", "stdout", "code_execution_output"),
        execution(
            "bash_code_execution_result",
            "stdout",
            "bash_code_execution_output",
        ),
        variant(
            "code_execution_tool_result_error",
            json!({"error_code":{"type":"string"}}),
            &["error_code"],
        ),
        variant(
            "bash_code_execution_tool_result_error",
            json!({"error_code":{"type":"string"}}),
            &["error_code"],
        ),
        variant(
            "text_editor_code_execution_tool_result_error",
            json!({"error_code":{"type":"string"}}),
            &["error_code"],
        ),
        variant(
            "text_editor_code_execution_view_result",
            json!({"content":{"type":"string"},"file_type":{"type":"string"},"num_lines":{"type":["number","null"]},"start_line":{"type":["number","null"]},"total_lines":{"type":["number","null"]}}),
            &[
                "content",
                "file_type",
                "num_lines",
                "start_line",
                "total_lines",
            ],
        ),
        variant(
            "text_editor_code_execution_create_result",
            json!({"is_file_update":{"type":"boolean"}}),
            &["is_file_update"],
        ),
        variant(
            "text_editor_code_execution_str_replace_result",
            json!({"lines":{"type":["array","null"],"items":{"type":"string"}},"new_lines":{"type":["number","null"]},"new_start":{"type":["number","null"]},"old_lines":{"type":["number","null"]},"old_start":{"type":["number","null"]}}),
            &["lines", "new_lines", "new_start", "old_lines", "old_start"],
        ),
    ];
    if kind == "code_execution_20260120" {
        variants.push(execution(
            "encrypted_code_execution_result",
            "encrypted_stdout",
            "code_execution_output",
        ));
    }
    Schema::from_json_schema(json!({"oneOf":variants}))
}

pub(super) fn tool(kind: &'static str) -> Tool {
    Tool::provider_executed(format!("anthropic.{kind}"), JsonObject::new())
        .input_schema(input_schema())
        .output_schema(output_schema(kind))
        .supports_deferred_results(true)
        .caller(ToolCallerDefinition::provider(move |options| {
            let mut options = options.unwrap_or_default();
            let anthropic = options.entry("anthropic".to_owned()).or_default();
            let mut callers = anthropic
                .get("allowedCallers")
                .and_then(JsonValue::as_array)
                .cloned()
                .unwrap_or_default();
            let caller = JsonValue::from(kind);
            if !callers.contains(&caller) {
                callers.push(caller);
            }
            anthropic.insert("allowedCallers".into(), JsonValue::Array(callers));
            options
        }))
        .build()
}
