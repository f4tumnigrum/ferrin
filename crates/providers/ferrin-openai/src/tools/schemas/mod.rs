//! Complete schemas for existing provider tool factories.

mod apply_patch;
mod code_interpreter;
mod computer;
mod custom;
mod file_search;
mod image_generation;
mod local_shell;
mod mcp;
mod programmatic_tool_calling;
mod shell;
mod tool_search;
mod web_search;
mod web_search_preview;

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ProviderError;
use ferrin_tool::Schema;

pub(crate) fn input(id: &str) -> Schema<JsonValue> {
    let value = match id {
        "openai.web_search" => web_search::input(),
        "openai.web_search_preview" => web_search_preview::input(),
        "openai.code_interpreter" => code_interpreter::input(),
        "openai.apply_patch" => apply_patch::input(),
        "openai.local_shell" => local_shell::input(),
        "openai.programmatic_tool_calling" => programmatic_tool_calling::input(),
        "openai.mcp" => mcp::input(),
        "openai.computer" => computer::input(),
        "openai.shell" => shell::input(),
        "openai.file_search" => file_search::input(),
        "openai.custom" => custom::input(),
        "openai.image_generation" => image_generation::input(),
        "openai.tool_search" => tool_search::input(),
        _ => return Schema::any(),
    };
    Schema::from_provider_json_schema(value)
}

pub(crate) fn output(id: &str) -> Option<Schema<JsonValue>> {
    let value = match id {
        "openai.web_search" => web_search::output(),
        "openai.web_search_preview" => web_search_preview::output(),
        "openai.code_interpreter" => code_interpreter::output(),
        "openai.apply_patch" => apply_patch::output(),
        "openai.local_shell" => local_shell::output(),
        "openai.programmatic_tool_calling" => programmatic_tool_calling::output(),
        "openai.mcp" => mcp::output(),
        "openai.computer" => computer::output(),
        "openai.shell" => shell::output(),
        "openai.file_search" => file_search::output(),
        "openai.image_generation" => image_generation::output(),
        "openai.tool_search" => tool_search::output(),
        _ => return None,
    };
    Some(Schema::from_provider_json_schema(value))
}

pub(crate) fn arguments(id: &str, arguments: &JsonObject) -> Result<JsonObject, ProviderError> {
    let schema = match id {
        "openai.web_search" => web_search::arguments(),
        "openai.web_search_preview" => web_search_preview::arguments(),
        "openai.code_interpreter" => code_interpreter::arguments(),
        "openai.apply_patch" => apply_patch::arguments(),
        "openai.mcp" => mcp::arguments(),
        "openai.shell" => shell::arguments(),
        "openai.file_search" => file_search::arguments(),
        "openai.custom" => custom::arguments(),
        "openai.image_generation" => image_generation::arguments(),
        "openai.tool_search" => tool_search::arguments(),
        _ => return Ok(arguments.clone()),
    };
    let value =
        Schema::from_provider_json_schema(schema).validate(JsonValue::Object(arguments.clone()))?;
    match value {
        JsonValue::Object(object) => Ok(object),
        _ => Ok(JsonObject::new()), // All extracted argument schemas require objects.
    }
}
