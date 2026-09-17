//! Deferred code-execution callers derived from Vercel AI SDK (Apache-2.0); see NOTICE.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_tool::Tool;
use ferrin_tool::ToolCallerDefinition;

pub(super) fn tool(kind: &'static str) -> Tool {
    let builder = Tool::provider_executed(format!("anthropic.{kind}"), JsonObject::new())
        .input_schema(super::schemas::input(&format!("anthropic.{kind}")))
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
        }));
    let builder = match super::schemas::output(&format!("anthropic.{kind}")) {
        Some(schema) => builder.output_schema(schema),
        None => builder,
    };
    builder.build()
}
