//! MCP request parameters and results used by the client.
//!
//! Every struct accepts unknown fields; the ones the client reads are typed
//! and the rest is kept in `extra` where callers may need it.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde::Deserialize;
use serde::Serialize;

/// Client or server implementation description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    /// Name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Display title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Elicitation capability (client or server side).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationCapability {
    /// Whether schema defaults are applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_defaults: Option<bool>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Capabilities the client declares.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Elicitation support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapability>,
    /// Other capabilities (`extensions`, `experimental`, ...).
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// A capability object with an optional `listChanged` flag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListCapability {
    /// Whether list-changed notifications are sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
    /// Whether subscriptions are supported (resources only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Capabilities the server declares.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Experimental capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<JsonObject>,
    /// Logging support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<JsonObject>,
    /// Argument completion support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completions: Option<JsonObject>,
    /// Prompts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<ListCapability>,
    /// Resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ListCapability>,
    /// Tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ListCapability>,
    /// Elicitation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapability>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Result of `server/discover`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverResult {
    /// Protocol versions the server supports.
    pub supported_versions: Vec<String>,
    /// Server capabilities.
    pub capabilities: ServerCapabilities,
    /// Instructions for the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Result metadata (`io.modelcontextprotocol/serverInfo`).
    #[serde(default, rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
    /// Other fields (`ttlMs`, `cacheScope`, `resultType`).
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Result of `initialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Negotiated protocol version.
    pub protocol_version: String,
    /// Server capabilities.
    pub capabilities: ServerCapabilities,
    /// Server implementation.
    pub server_info: Implementation,
    /// Instructions for the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Behavioural hints of a tool (untrusted unless the server is).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    /// Display title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The tool does not modify its environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// The tool may perform destructive updates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// Repeated calls have no additional effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// The tool interacts with external entities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// A tool definition returned by `tools/list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    /// Name.
    pub name: String,
    /// Display title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema of the arguments.
    pub input_schema: JsonObject,
    /// JSON Schema of `structuredContent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<JsonObject>,
    /// Hints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
    /// Tool metadata (`_meta`).
    #[serde(default, rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Result of `tools/list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    /// Tools.
    pub tools: Vec<McpTool>,
    /// Cursor of the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Contents of a resource (`text` or `blob`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContents {
    /// URI.
    pub uri: String,
    /// Name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Text contents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Base64 binary contents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    /// Metadata (`_meta`).
    #[serde(default, rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// One content item of a tool result or prompt message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Content {
    /// Text.
    Text {
        /// The text.
        text: String,
        /// Other fields (`annotations`, `_meta`).
        #[serde(flatten)]
        extra: JsonObject,
    },
    /// Base64 image.
    Image {
        /// Base64 data.
        data: String,
        /// MIME type.
        #[serde(rename = "mimeType")]
        mime_type: String,
        /// Other fields.
        #[serde(flatten)]
        extra: JsonObject,
    },
    /// Base64 audio.
    Audio {
        /// Base64 data.
        data: String,
        /// MIME type.
        #[serde(rename = "mimeType")]
        mime_type: String,
        /// Other fields.
        #[serde(flatten)]
        extra: JsonObject,
    },
    /// Embedded resource.
    Resource {
        /// The resource contents.
        resource: ResourceContents,
        /// Other fields.
        #[serde(flatten)]
        extra: JsonObject,
    },
    /// Link to a resource.
    ResourceLink {
        /// URI.
        uri: String,
        /// Name.
        name: String,
        /// Description.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// MIME type.
        #[serde(default, rename = "mimeType", skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        /// Other fields.
        #[serde(flatten)]
        extra: JsonObject,
    },
}

/// Result of `tools/call`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    /// Unstructured content.
    #[serde(default)]
    pub content: Vec<JsonValue>,
    /// Structured content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    /// Whether the call failed.
    #[serde(default)]
    pub is_error: bool,
    /// Other fields (`toolResult`, `_meta`).
    #[serde(flatten)]
    pub extra: JsonObject,
}

impl CallToolResult {
    /// Typed view of the content items; unknown item types are skipped.
    #[must_use]
    pub fn typed_content(&self) -> Vec<Content> {
        self.content
            .iter()
            .filter_map(|item| serde_json::from_value(item.clone()).ok())
            .collect()
    }

    /// Concatenated text of the `text` items.
    #[must_use]
    pub fn text(&self) -> String {
        self.typed_content()
            .into_iter()
            .filter_map(|item| match item {
                Content::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// The whole result as a JSON object.
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        serde_json::to_value(self).unwrap_or(JsonValue::Null)
    }
}

/// A resource descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    /// URI.
    pub uri: String,
    /// Name.
    pub name: String,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Result of `resources/list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResourcesResult {
    /// Resources.
    pub resources: Vec<Resource>,
    /// Cursor of the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// A resource template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTemplate {
    /// URI template.
    pub uri_template: String,
    /// Name.
    pub name: String,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Result of `resources/templates/list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResourceTemplatesResult {
    /// Templates.
    pub resource_templates: Vec<ResourceTemplate>,
    /// Cursor of the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Result of `resources/read`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadResourceResult {
    /// Contents.
    pub contents: Vec<ResourceContents>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// A prompt argument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptArgument {
    /// Name.
    pub name: String,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the argument is required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// A prompt descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    /// Name.
    pub name: String,
    /// Title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Result of `prompts/list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPromptsResult {
    /// Prompts.
    pub prompts: Vec<Prompt>,
    /// Cursor of the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// A prompt message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptMessage {
    /// Role (`user` or `assistant`).
    pub role: String,
    /// Content.
    pub content: Content,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Result of `prompts/get`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetPromptResult {
    /// Description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Messages.
    pub messages: Vec<PromptMessage>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Reference completed by `completion/complete`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum CompletionReference {
    /// A prompt argument.
    #[serde(rename = "ref/prompt")]
    Prompt {
        /// Prompt name.
        name: String,
    },
    /// A resource template argument.
    #[serde(rename = "ref/resource")]
    Resource {
        /// Resource URI or template.
        uri: String,
    },
}

/// Parameters of `completion/complete`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteParams {
    /// What is completed.
    #[serde(rename = "ref")]
    pub reference: CompletionReference,
    /// The argument being completed.
    pub argument: CompletionArgument,
    /// Previously resolved arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<CompletionContext>,
}

/// The argument being completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionArgument {
    /// Argument name.
    pub name: String,
    /// Current value.
    pub value: String,
}

/// Context of a completion request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionContext {
    /// Already resolved arguments.
    #[serde(default)]
    pub arguments: std::collections::BTreeMap<String, String>,
}

/// Result of `completion/complete`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteResult {
    /// Completion values.
    pub completion: Completion,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Completion values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    /// Values (at most 100).
    pub values: Vec<String>,
    /// Total number of matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Whether more values exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    /// Other fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// An `elicitation/create` request from the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationRequest {
    /// Message shown to the user.
    pub message: String,
    /// Schema of the requested content (not validated by the client).
    #[serde(default)]
    pub requested_schema: JsonValue,
    /// Other fields (`mode`, `url`, ...).
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// User decision of an elicitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ElicitAction {
    /// The user provided the content.
    Accept,
    /// The user declined.
    Decline,
    /// The user cancelled.
    Cancel,
}

/// Result of an elicitation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElicitResult {
    /// Decision.
    pub action: ElicitAction,
    /// Content, when accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<JsonObject>,
}

impl ElicitResult {
    /// An accepted elicitation with `content`.
    #[must_use]
    pub fn accept(content: JsonObject) -> Self {
        Self {
            action: ElicitAction::Accept,
            content: Some(content),
        }
    }

    /// A declined elicitation.
    #[must_use]
    pub fn decline() -> Self {
        Self {
            action: ElicitAction::Decline,
            content: None,
        }
    }

    /// A cancelled elicitation.
    #[must_use]
    pub fn cancel() -> Self {
        Self {
            action: ElicitAction::Cancel,
            content: None,
        }
    }
}
