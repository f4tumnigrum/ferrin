//! Response-level provider metadata (`usage`, `stopSequence`, `container`,
//! `contextManagement`, ...).

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderMetadata;
use serde_json::json;

use super::anthropic_metadata;
use crate::api_types::AppliedEdit;
use crate::api_types::ContainerInfo;
use crate::api_types::ResponseContextManagement;
use crate::api_types::StopDetails;
use crate::api_types::UsageIteration;
use crate::config::CANONICAL_OPTIONS_KEY;

/// Renders container information (`{expiresAt, id, skills}`).
#[must_use]
pub fn container_metadata(container: &ContainerInfo, include_skills: bool) -> JsonValue {
    let skills = if include_skills {
        container.skills.as_ref().map(|skills| {
            JsonValue::Array(
                skills
                    .iter()
                    .map(|skill| {
                        json!({
                            "type": skill.kind,
                            "skillId": skill.skill_id,
                            "version": skill.version,
                        })
                    })
                    .collect(),
            )
        })
    } else {
        None
    };
    json!({
        "expiresAt": container.expires_at,
        "id": container.id,
        "skills": skills.unwrap_or(JsonValue::Null),
    })
}

fn applied_edit(edit: &AppliedEdit) -> Option<JsonValue> {
    match edit.kind.as_str() {
        "clear_tool_uses_20250919" => Some(json!({
            "type": edit.kind,
            "clearedToolUses": edit.cleared_tool_uses,
            "clearedInputTokens": edit.cleared_input_tokens,
        })),
        "clear_thinking_20251015" => Some(json!({
            "type": edit.kind,
            "clearedThinkingTurns": edit.cleared_thinking_turns,
            "clearedInputTokens": edit.cleared_input_tokens,
        })),
        "compact_20260112" => Some(json!({"type": edit.kind})),
        _ => None,
    }
}

/// Renders context management results (`{appliedEdits: [...]}`).
#[must_use]
pub fn context_management_metadata(context: &ResponseContextManagement) -> JsonValue {
    json!({
        "appliedEdits": context
            .applied_edits
            .iter()
            .filter_map(applied_edit)
            .collect::<Vec<_>>(),
    })
}

fn stop_details_metadata(details: &StopDetails) -> JsonValue {
    let mut value = JsonObject::new();
    value.insert("type".to_owned(), JsonValue::from(details.kind.clone()));
    if let Some(category) = &details.category {
        value.insert("category".to_owned(), JsonValue::from(category.clone()));
    }
    if let Some(explanation) = &details.explanation {
        value.insert(
            "explanation".to_owned(),
            JsonValue::from(explanation.clone()),
        );
    }
    if let Some(model) = &details.recommended_model {
        value.insert(
            "recommendedModel".to_owned(),
            JsonValue::from(model.clone()),
        );
    }
    JsonValue::Object(value)
}

fn iteration_metadata(iteration: &UsageIteration) -> JsonValue {
    let mut value = JsonObject::new();
    value.insert("type".to_owned(), JsonValue::from(iteration.kind.clone()));
    if let Some(model) = &iteration.model {
        value.insert("model".to_owned(), JsonValue::from(model.clone()));
    }
    value.insert(
        "inputTokens".to_owned(),
        JsonValue::from(iteration.input_tokens.unwrap_or(0)),
    );
    value.insert(
        "outputTokens".to_owned(),
        JsonValue::from(iteration.output_tokens.unwrap_or(0)),
    );
    if let Some(cache) = iteration
        .cache_creation_input_tokens
        .filter(|count| *count > 0)
    {
        value.insert(
            "cacheCreationInputTokens".to_owned(),
            JsonValue::from(cache),
        );
    }
    if let Some(cache) = iteration.cache_read_input_tokens.filter(|count| *count > 0) {
        value.insert("cacheReadInputTokens".to_owned(), JsonValue::from(cache));
    }
    JsonValue::Object(value)
}

/// Inputs of the response-level provider metadata.
#[derive(Debug, Default)]
pub struct MessageMetadata<'a> {
    /// Raw usage object as returned by the API.
    pub usage: Option<JsonObject>,
    /// Stop sequence.
    pub stop_sequence: Option<String>,
    /// Stop details.
    pub stop_details: Option<&'a StopDetails>,
    /// Input transformations.
    pub input_transformations: Option<&'a JsonValue>,
    /// Per-iteration usage.
    pub iterations: Option<&'a [UsageIteration]>,
    /// Rendered container information (see [`container_metadata`]).
    pub container: Option<JsonValue>,
    /// Context management results.
    pub context_management: Option<&'a ResponseContextManagement>,
}

impl MessageMetadata<'_> {
    /// Builds the metadata under `anthropic` and, when `custom_key` is set,
    /// under that key as well.
    #[must_use]
    pub fn build(self, custom_key: Option<&str>) -> ProviderMetadata {
        let mut value = JsonObject::new();
        value.insert(
            "usage".to_owned(),
            self.usage.map_or(JsonValue::Null, JsonValue::Object),
        );
        value.insert(
            "stopSequence".to_owned(),
            self.stop_sequence.map_or(JsonValue::Null, JsonValue::from),
        );
        if let Some(details) = self.stop_details {
            value.insert("stopDetails".to_owned(), stop_details_metadata(details));
        }
        if let Some(transformations) = self.input_transformations {
            value.insert("inputTransformations".to_owned(), transformations.clone());
        }
        value.insert(
            "iterations".to_owned(),
            self.iterations.map_or(JsonValue::Null, |iterations| {
                JsonValue::Array(iterations.iter().map(iteration_metadata).collect())
            }),
        );
        value.insert(
            "container".to_owned(),
            self.container.unwrap_or(JsonValue::Null),
        );
        value.insert(
            "contextManagement".to_owned(),
            self.context_management
                .map_or(JsonValue::Null, context_management_metadata),
        );
        let mut metadata = anthropic_metadata(value.clone());
        if let Some(key) = custom_key.filter(|key| *key != CANONICAL_OPTIONS_KEY) {
            metadata.insert(key.to_owned(), value);
        }
        metadata
    }
}
