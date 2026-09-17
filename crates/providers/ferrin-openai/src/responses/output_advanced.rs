//! Advanced Responses tool items, shared by complete and streamed responses.
//!
//! Protocol behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use ferrin_spec::JsonValue;
use ferrin_spec::language_model::Content;
use ferrin_spec::language_model::ProviderToolResult;
use ferrin_spec::language_model::ToolCall;
use ferrin_spec::language_model::ToolDefinition;
use serde_json::json;

use super::api_types::OutputItem;
use super::output::OutputMapper;

impl OutputMapper {
    pub(crate) fn configure_tools(&mut self, tools: &[ToolDefinition]) {
        for tool in tools {
            match tool {
                ToolDefinition::Function { name, .. } => {
                    self.function_names.insert(name.as_str().to_owned());
                }
                ToolDefinition::Provider { id, args, .. } if id == "openai.shell" => {
                    self.hosted_shell = args
                        .get("environment")
                        .and_then(|value| value.get("type"))
                        .and_then(JsonValue::as_str)
                        .is_some_and(|kind| kind != "local");
                }
                _ => {}
            }
        }
    }

    pub(super) fn advanced_item(&mut self, item: &OutputItem) -> Vec<Content> {
        let id = item.get_str("call_id").unwrap_or_else(|| item.id_str());
        let (name, value, executed) = match item.kind.as_str() {
            "program" => (
                "programmatic_tool_calling",
                json!({"code": item.get("code"), "fingerprint": item.get("fingerprint")}),
                true,
            ),
            "tool_search_call" => {
                let hosted = item.get_str("execution") == Some("server");
                if hosted {
                    self.hosted_search_ids.push_back(id.to_owned());
                }
                self.has_function_call |= !hosted;
                (
                    "tool_search",
                    json!({"arguments": item.get("arguments"), "call_id": item.get("call_id")}),
                    hosted,
                )
            }
            "program_output" => {
                return vec![self.advanced_result(
                    item,
                    id,
                    "programmatic_tool_calling",
                    json!({"result": item.get("result"), "status": item.status}),
                )];
            }
            "tool_search_output" => {
                let id = item
                    .get_str("call_id")
                    .map(str::to_owned)
                    .or_else(|| self.hosted_search_ids.pop_front())
                    .unwrap_or_else(|| item.id_str().to_owned());
                return vec![self.advanced_result(
                    item,
                    &id,
                    "tool_search",
                    json!({"tools": item.get("tools")}),
                )];
            }
            "shell_call_output" => {
                let mut output = item.get("output").cloned().unwrap_or_else(|| json!([]));
                if let Some(entries) = output.as_array_mut() {
                    for entry in entries {
                        if let Some(outcome) =
                            entry.get_mut("outcome").and_then(JsonValue::as_object_mut)
                            && let Some(code) = outcome.remove("exit_code")
                        {
                            outcome.insert("exitCode".into(), code);
                        }
                    }
                }
                return vec![self.advanced_result(item, id, "shell", json!({"output": output}))];
            }
            _ => return Vec::new(),
        };
        let mut call = ToolCall::new(id, self.custom_name(name), value.to_string());
        call.provider_executed = executed;
        call.provider_metadata = Some(self.item_metadata(item));
        vec![Content::ToolCall(call)]
    }

    fn advanced_result(
        &self,
        item: &OutputItem,
        id: &str,
        name: &str,
        result: JsonValue,
    ) -> Content {
        Content::ToolResult(ProviderToolResult {
            tool_call_id: id.into(),
            tool_name: self.custom_name(name).into(),
            result,
            is_error: false,
            preliminary: false,
            dynamic: false,
            provider_metadata: Some(self.item_metadata(item)),
        })
    }
}
