//! Conversion of the specification prompt into Messages API `system` and
//! `messages` fields.
//!
//! Consecutive messages are grouped by role: the first system group becomes
//! the top-level `system` array, later system groups become inline system
//! messages (beta), user and tool messages share one `user` group and
//! assistant messages form `assistant` groups.

mod assistant;
mod provider_results;
mod user;

use std::collections::BTreeSet;

use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::PromptMessage;
use serde_json::json;

use crate::cache_control::CacheControlValidator;
use crate::config::AnthropicConfig;
use crate::options::SystemMessageOptions;
use crate::options::part_options;
use crate::options::read_options;

/// Beta enabling `role: system` messages after the first turn.
pub const BETA_MID_CONVERSATION_SYSTEM: &str = "mid-conversation-system-2026-04-07";
/// Beta enabling `tool_changes` on inline system messages.
pub const BETA_MID_CONVERSATION_TOOL_CHANGES: &str = "mid-conversation-tool-changes-2026-07-01";
/// Beta enabling `clear_at` on inline system messages.
pub const BETA_MID_CONVERSATION_CLEAR_AT: &str = "mid-conversation-system-clear-at-2026-08-21";
/// Beta enabling `output_config.effort` on inline system messages.
pub const BETA_MID_CONVERSATION_EFFORT: &str = "mid-conversation-effort-2026-08-01";
/// Beta enabling file references.
pub const BETA_FILES_API: &str = "files-api-2025-04-14";
/// Beta enabling PDF documents.
pub const BETA_PDFS: &str = "pdfs-2024-09-25";

/// Result of the prompt conversion.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedPrompt {
    /// Top-level `system` blocks.
    pub system: Option<Vec<JsonValue>>,
    /// `messages`.
    pub messages: Vec<JsonValue>,
    /// Beta flags required by the prompt.
    pub betas: BTreeSet<String>,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Shared conversion state.
pub(crate) struct Converter<'a> {
    pub(crate) config: &'a AnthropicConfig,
    pub(crate) mapping: &'a ToolNameMapping,
    pub(crate) send_reasoning: bool,
    pub(crate) cache: &'a mut CacheControlValidator,
    pub(crate) betas: BTreeSet<String>,
    pub(crate) warnings: Vec<Warning>,
}

impl Converter<'_> {
    /// Cache control for a part, falling back to the message level for the
    /// last part of a message.
    pub(crate) fn cache_control(
        &mut self,
        part_options: Option<&ProviderOptions>,
        message_options: Option<&ProviderOptions>,
        is_last_part: bool,
        context: &str,
        can_cache: bool,
    ) -> Option<JsonValue> {
        if let Some(value) = self
            .cache
            .get(self.config, part_options, context, can_cache)
        {
            return Some(value);
        }
        if is_last_part {
            return self
                .cache
                .get(self.config, message_options, context, can_cache);
        }
        None
    }

    pub(crate) fn warn_unsupported(&mut self, feature: impl Into<String>) {
        self.warnings.push(Warning::unsupported(feature));
    }

    pub(crate) fn warn_other(&mut self, message: impl Into<String>) {
        self.warnings.push(Warning::other(message));
    }

    /// Raw option object of a part or message.
    pub(crate) fn options<'o>(
        &self,
        options: Option<&'o ProviderOptions>,
    ) -> Option<&'o JsonObject> {
        part_options(self.config, options)
    }
}

/// Inserts `cache_control` into a block when set.
pub(crate) fn with_cache_control(
    mut block: JsonValue,
    cache_control: Option<JsonValue>,
) -> JsonValue {
    if let (Some(cache_control), Some(object)) = (cache_control, block.as_object_mut()) {
        object.insert("cache_control".to_owned(), cache_control);
    }
    block
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    System,
    User,
    Assistant,
}

fn role_of(message: &PromptMessage) -> Role {
    match message {
        PromptMessage::System { .. } => Role::System,
        PromptMessage::User { .. } | PromptMessage::Tool { .. } => Role::User,
        PromptMessage::Assistant { .. } => Role::Assistant,
        #[allow(unreachable_patterns, reason = "PromptMessage is non-exhaustive")]
        _ => Role::User,
    }
}

/// Groups consecutive messages of the same role.
fn group_by_role(prompt: &[PromptMessage]) -> Vec<(Role, Vec<&PromptMessage>)> {
    let mut groups: Vec<(Role, Vec<&PromptMessage>)> = Vec::new();
    for message in prompt {
        let role = role_of(message);
        match groups.last_mut() {
            Some((last_role, messages)) if *last_role == role => messages.push(message),
            _ => groups.push((role, vec![message])),
        }
    }
    groups
}

/// Converts `prompt`.
///
/// # Errors
///
/// Returns [`ProviderError::UnsupportedFunctionality`] for file media types
/// the API does not accept and [`ProviderError::NoSuchProviderReference`]
/// for file references of another provider.
pub fn convert_prompt(
    config: &AnthropicConfig,
    prompt: &[PromptMessage],
    mapping: &ToolNameMapping,
    send_reasoning: bool,
) -> Result<ConvertedPrompt, ProviderError> {
    let mut cache = CacheControlValidator::new();
    let mut converted =
        convert_prompt_with_cache(config, prompt, mapping, send_reasoning, &mut cache)?;
    converted.warnings.extend(cache.into_warnings());
    Ok(converted)
}

pub(crate) fn convert_prompt_with_cache(
    config: &AnthropicConfig,
    prompt: &[PromptMessage],
    mapping: &ToolNameMapping,
    send_reasoning: bool,
    cache: &mut CacheControlValidator,
) -> Result<ConvertedPrompt, ProviderError> {
    let mut converter = Converter {
        config,
        mapping,
        send_reasoning,
        cache,
        betas: BTreeSet::new(),
        warnings: Vec::new(),
    };
    let groups = group_by_role(prompt);
    let mut system: Option<Vec<JsonValue>> = None;
    let mut messages: Vec<JsonValue> = Vec::new();
    let group_count = groups.len();
    for (index, (role, group)) in groups.into_iter().enumerate() {
        let is_last_group = index + 1 == group_count;
        match role {
            Role::System => {
                if system.is_none() && messages.is_empty() {
                    system = Some(converter.top_level_system(&group));
                } else {
                    messages.push(converter.inline_system(&group));
                }
            }
            Role::User => {
                let content = user::convert_user_group(&mut converter, &group)?;
                messages.push(json!({"role": "user", "content": content}));
            }
            Role::Assistant => {
                let content =
                    assistant::convert_assistant_group(&mut converter, &group, is_last_group);
                messages.push(json!({"role": "assistant", "content": content}));
            }
        }
    }
    let Converter {
        betas, warnings, ..
    } = converter;
    Ok(ConvertedPrompt {
        system,
        messages,
        betas,
        warnings,
    })
}

impl Converter<'_> {
    fn system_blocks(&mut self, group: &[&PromptMessage]) -> Vec<JsonValue> {
        let mut blocks = Vec::new();
        for message in group {
            let PromptMessage::System {
                content,
                provider_options,
            } = message
            else {
                continue;
            };
            let cache_control = self.cache.get(
                self.config,
                provider_options.as_ref(),
                "system message",
                true,
            );
            blocks.push(with_cache_control(
                json!({"type": "text", "text": content}),
                cache_control,
            ));
        }
        blocks
    }

    fn top_level_system(&mut self, group: &[&PromptMessage]) -> Vec<JsonValue> {
        for message in group {
            let options: Option<SystemMessageOptions> =
                read_options(self.options(message.provider_options()));
            let Some(options) = options else {
                continue;
            };
            if options.tool_changes.is_some() {
                self.warn_other(
                    "toolChanges on the first system message is ignored; the leading system \
                     prompt is sent as the top-level system field",
                );
            }
            if options.clear_at.is_some() {
                self.warn_other(
                    "clearAt on the first system message is ignored; the leading system prompt \
                     is sent as the top-level system field",
                );
            }
            if options.effort.is_some() {
                self.warn_other(
                    "effort on the first system message is ignored; the leading system prompt \
                     is sent as the top-level system field",
                );
            }
        }
        self.system_blocks(group)
    }

    fn inline_system(&mut self, group: &[&PromptMessage]) -> JsonValue {
        self.betas.insert(BETA_MID_CONVERSATION_SYSTEM.to_owned());
        let content = self.system_blocks(group);
        let mut message = JsonObject::new();
        message.insert("role".to_owned(), JsonValue::from("system"));
        message.insert("content".to_owned(), JsonValue::Array(content));
        let mut clear_at: Option<String> = None;
        let mut effort: Option<String> = None;
        let mut tool_changes: Vec<JsonValue> = Vec::new();
        for entry in group {
            let options: Option<SystemMessageOptions> =
                read_options(self.options(entry.provider_options()));
            let Some(options) = options else {
                continue;
            };
            if options.clear_at.is_some() {
                clear_at = options.clear_at;
            }
            if options.effort.is_some() {
                effort = options.effort;
            }
            if let Some(changes) = options.tool_changes {
                tool_changes.extend(
                    changes
                        .iter()
                        .map(|change| json!({"type": change.kind, "tool_name": change.tool_name})),
                );
            }
        }
        if let Some(clear_at) = clear_at {
            self.betas.insert(BETA_MID_CONVERSATION_CLEAR_AT.to_owned());
            message.insert("clear_at".to_owned(), JsonValue::from(clear_at));
        }
        if let Some(effort) = effort {
            self.betas.insert(BETA_MID_CONVERSATION_EFFORT.to_owned());
            message.insert("output_config".to_owned(), json!({"effort": effort}));
        }
        if !tool_changes.is_empty() {
            self.betas
                .insert(BETA_MID_CONVERSATION_TOOL_CHANGES.to_owned());
            message.insert("tool_changes".to_owned(), JsonValue::Array(tool_changes));
        }
        JsonValue::Object(message)
    }
}
