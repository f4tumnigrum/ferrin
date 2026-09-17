//! Advanced tool schemas and provider caller binding.
//!
//! Protocol behavior adapted from the Vercel AI SDK (Apache-2.0); see NOTICE.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_tool::Tool;
use ferrin_tool::ToolCallerDefinition;

pub(super) fn programmatic() -> Tool {
    let builder = Tool::provider_executed("openai.programmatic_tool_calling", JsonObject::new())
        .input_schema(super::schemas::input("openai.programmatic_tool_calling"))
        .supports_deferred_results(true)
        .caller(ToolCallerDefinition::provider(|options| {
            let mut options = options.unwrap_or_default();
            let openai = options.entry("openai".to_owned()).or_default();
            let mut callers = openai
                .get("allowedCallers")
                .and_then(JsonValue::as_array)
                .cloned()
                .unwrap_or_default();
            let caller = JsonValue::from("programmatic");
            if !callers.contains(&caller) {
                callers.push(caller);
            }
            openai.insert("allowedCallers".into(), JsonValue::Array(callers));
            options
        }));
    bind_output(builder, "openai.programmatic_tool_calling")
}

fn bind_output(mut builder: ferrin_tool::ToolBuilder<JsonValue>, id: &str) -> Tool {
    if let Some(output) = super::schemas::output(id) {
        builder = builder.output_schema(output);
    }
    builder.build()
}

pub(super) fn search(arguments: JsonObject) -> Tool {
    let builder = if arguments.get("execution").and_then(JsonValue::as_str) == Some("client") {
        Tool::provider_defined("openai.tool_search", arguments)
    } else {
        Tool::provider_executed("openai.tool_search", arguments)
    };
    bind_output(
        builder.input_schema(super::schemas::input("openai.tool_search")),
        "openai.tool_search",
    )
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
    bind_output(
        builder.input_schema(super::schemas::input("openai.shell")),
        "openai.shell",
    )
}
