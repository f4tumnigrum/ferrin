//! Complete schemas for existing provider tool factories.

mod advisor_20260301;
mod bash_20241022;
mod bash_20250124;
mod code_execution_20250522;
mod code_execution_20250825;
mod code_execution_20260120;
mod computer_20241022;
mod computer_20250124;
mod computer_20251124;
mod memory_20250818;
mod text_editor_20241022;
mod text_editor_20250124;
mod text_editor_20250429;
mod text_editor_20250728;
mod tool_search_bm25_20251119;
mod tool_search_regex_20251119;
mod web_fetch_20250910;
mod web_fetch_20260209;
mod web_search_20250305;
mod web_search_20260209;

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ProviderError;
use ferrin_tool::Schema;

pub(crate) fn input(id: &str) -> Schema<JsonValue> {
    let value = match id {
        "anthropic.text_editor_20250728" => text_editor_20250728::input(),
        "anthropic.web_fetch_20250910" => web_fetch_20250910::input(),
        "anthropic.tool_search_bm25_20251119" => tool_search_bm25_20251119::input(),
        "anthropic.code_execution_20250522" => code_execution_20250522::input(),
        "anthropic.web_fetch_20260209" => web_fetch_20260209::input(),
        "anthropic.bash_20241022" => bash_20241022::input(),
        "anthropic.web_search_20250305" => web_search_20250305::input(),
        "anthropic.computer_20241022" => computer_20241022::input(),
        "anthropic.advisor_20260301" => advisor_20260301::input(),
        "anthropic.web_search_20260209" => web_search_20260209::input(),
        "anthropic.code_execution_20250825" => code_execution_20250825::input(),
        "anthropic.code_execution_20260120" => code_execution_20260120::input(),
        "anthropic.bash_20250124" => bash_20250124::input(),
        "anthropic.computer_20250124" => computer_20250124::input(),
        "anthropic.text_editor_20241022" => text_editor_20241022::input(),
        "anthropic.text_editor_20250429" => text_editor_20250429::input(),
        "anthropic.computer_20251124" => computer_20251124::input(),
        "anthropic.memory_20250818" => memory_20250818::input(),
        "anthropic.text_editor_20250124" => text_editor_20250124::input(),
        "anthropic.tool_search_regex_20251119" => tool_search_regex_20251119::input(),
        _ => return Schema::any(),
    };
    Schema::from_provider_json_schema(value)
}

pub(crate) fn output(id: &str) -> Option<Schema<JsonValue>> {
    let value = match id {
        "anthropic.web_fetch_20250910" => web_fetch_20250910::output(),
        "anthropic.tool_search_bm25_20251119" => tool_search_bm25_20251119::output(),
        "anthropic.code_execution_20250522" => code_execution_20250522::output(),
        "anthropic.web_fetch_20260209" => web_fetch_20260209::output(),
        "anthropic.web_search_20250305" => web_search_20250305::output(),
        "anthropic.advisor_20260301" => advisor_20260301::output(),
        "anthropic.web_search_20260209" => web_search_20260209::output(),
        "anthropic.code_execution_20250825" => code_execution_20250825::output(),
        "anthropic.code_execution_20260120" => code_execution_20260120::output(),
        "anthropic.tool_search_regex_20251119" => tool_search_regex_20251119::output(),
        _ => return None,
    };
    Some(Schema::from_provider_json_schema(value))
}

pub(crate) fn arguments(id: &str, arguments: &JsonObject) -> Result<JsonObject, ProviderError> {
    let schema = match id {
        "anthropic.text_editor_20250728" => text_editor_20250728::arguments(),
        "anthropic.web_fetch_20250910" => web_fetch_20250910::arguments(),
        "anthropic.web_fetch_20260209" => web_fetch_20260209::arguments(),
        "anthropic.web_search_20250305" => web_search_20250305::arguments(),
        "anthropic.advisor_20260301" => advisor_20260301::arguments(),
        "anthropic.web_search_20260209" => web_search_20260209::arguments(),
        _ => return Ok(arguments.clone()),
    };
    let value =
        Schema::from_provider_json_schema(schema).validate(JsonValue::Object(arguments.clone()))?;
    match value {
        JsonValue::Object(object) => Ok(object),
        _ => Ok(JsonObject::new()), // All extracted argument schemas require objects.
    }
}
