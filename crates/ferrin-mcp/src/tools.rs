//! Bridging MCP tools into a [`ToolSet`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use bytes::Bytes;
use ferrin_schema::Schema;
use ferrin_spec::FileData;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::language_model::prompt::ToolResultContentPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_tool::ModelOutputArgs;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolExecute;
use ferrin_tool::ToolOutput;
use ferrin_tool::ToolOutputStream;
use ferrin_tool::ToolSet;
use serde_json::json;

use crate::apps::app_tool_meta;
use crate::client::McpClient;
use crate::client::RequestOptions;
use crate::error::McpError;
use crate::protocol::CallToolResult;
use crate::protocol::McpTool;
use crate::transport::HeaderBinding;
use crate::transport::header_bindings;
use crate::transport::tool_headers;

/// Input and output schema of one tool.
#[derive(Debug, Clone)]
pub struct ToolSchemaPair {
    /// Input schema.
    pub input: Schema<JsonValue>,
    /// Output schema; when set, `structuredContent` (or the first text
    /// content parsed as JSON) is validated against it.
    pub output: Option<Schema<JsonValue>>,
}

impl ToolSchemaPair {
    /// A pair with only an input schema.
    #[must_use]
    pub fn input(input: Schema<JsonValue>) -> Self {
        Self {
            input,
            output: None,
        }
    }
}

/// Where tool schemas come from.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub enum ToolSchemas {
    /// Use the server's `inputSchema`; every tool becomes a dynamic tool.
    #[default]
    Automatic,
    /// Use caller-provided schemas; only the listed tools are included and
    /// they become function tools.
    Explicit(HashMap<String, ToolSchemaPair>),
}

/// Options of [`McpClient::tools`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ToolsOptions {
    /// Schema source.
    pub schemas: ToolSchemas,
    /// Timeout of each `tools/call` (and of the `tools/list` requests).
    pub request_timeout: Option<Duration>,
    /// Prefix added to every tool name in the returned set.
    pub name_prefix: Option<String>,
}

impl ToolsOptions {
    /// Explicit schemas for the listed tools.
    #[must_use]
    pub fn explicit(schemas: HashMap<String, ToolSchemaPair>) -> Self {
        Self {
            schemas: ToolSchemas::Explicit(schemas),
            ..Self::default()
        }
    }

    /// Sets the request timeout.
    #[must_use]
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Sets the name prefix.
    #[must_use]
    pub fn name_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.name_prefix = Some(prefix.into());
        self
    }
}

/// Executes one MCP tool through a client.
#[derive(Clone)]
pub struct McpToolExecutor {
    client: McpClient,
    tool_name: String,
    request_timeout: Option<Duration>,
    header_bindings: Arc<[HeaderBinding]>,
    output_schema: Option<Schema<JsonValue>>,
}

impl std::fmt::Debug for McpToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpToolExecutor")
            .field("tool_name", &self.tool_name)
            .field("request_timeout", &self.request_timeout)
            .field("header_bindings", &self.header_bindings)
            .finish_non_exhaustive()
    }
}

fn to_tool_error(error: McpError) -> ToolError {
    match error {
        McpError::Cancelled => ToolError::Cancelled,
        McpError::Timeout(duration) => ToolError::Timeout(duration),
        other => ToolError::from_error(other),
    }
}

impl McpToolExecutor {
    /// Extracts the structured output validated against `schema`.
    fn structured_output(
        result: &CallToolResult,
        schema: &Schema<JsonValue>,
    ) -> Result<JsonValue, ToolError> {
        let candidate = match &result.structured_content {
            Some(content) => content.clone(),
            None => {
                let text = result
                    .typed_content()
                    .into_iter()
                    .find_map(|content| match content {
                        crate::protocol::Content::Text { text, .. } => Some(text),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        ToolError::message(
                            "tool result has no structuredContent and no text content",
                        )
                    })?;
                serde_json::from_str(&text).map_err(|error| {
                    ToolError::message(format!("tool result text is not valid JSON: {error}"))
                })?
            }
        };
        schema.validate(candidate).map_err(|error| {
            ToolError::message(format!(
                "tool result does not match its output schema: {error}"
            ))
        })
    }

    async fn run(self, input: JsonValue, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let arguments = match input {
            JsonValue::Object(object) => object,
            JsonValue::Null => JsonObject::new(),
            _ => return Err(ToolError::message("MCP tool input must be a JSON object")),
        };
        let mut headers = Headers::new();
        if !self.header_bindings.is_empty() {
            for (name, value) in
                tool_headers(&self.header_bindings, &arguments).map_err(to_tool_error)?
            {
                headers.insert(&name, &value).map_err(|error| {
                    ToolError::message(format!("invalid tool header {name}: {error}"))
                })?;
            }
        }
        let options = RequestOptions {
            timeout: self.request_timeout,
            max_total_timeout: None,
            cancellation: Some(ctx.cancellation.clone()),
            headers,
        };
        let result = self
            .client
            .call_tool(&self.tool_name, Some(arguments), options)
            .await
            .map_err(to_tool_error)?;
        if result.is_error {
            return Err(ToolError::json(result.to_json()));
        }
        let output = match &self.output_schema {
            Some(schema) => Self::structured_output(&result, schema)?,
            None => result.to_json(),
        };
        Ok(ToolOutput::Final(output))
    }
}

impl ToolExecute for McpToolExecutor {
    fn execute(&self, input: JsonValue, ctx: ToolContext) -> ToolOutputStream {
        let executor = self.clone();
        Box::pin(futures_util::stream::once(executor.run(input, ctx)))
    }
}

fn base64_file(item: &JsonObject, default_media_type: &str) -> Option<ToolResultContentPart> {
    let data = item.get("data")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let media_type = item
        .get("mimeType")
        .and_then(JsonValue::as_str)
        .unwrap_or(default_media_type);
    Some(ToolResultContentPart::File {
        data: FileData::Bytes {
            data: Bytes::from(bytes),
        },
        media_type: MediaType::new(media_type),
        filename: None,
        provider_options: None,
    })
}

/// Converts a tool output into what the model receives: MCP content arrays
/// become multi-part content (text, image and audio files), everything else
/// is passed as JSON.
#[must_use]
pub fn mcp_to_model_output(args: ModelOutputArgs<'_>) -> ToolResultOutput {
    let Some(content) = args.output.get("content").and_then(JsonValue::as_array) else {
        return ToolResultOutput::json(args.output.clone());
    };
    let parts = content
        .iter()
        .map(|item| {
            let object = item.as_object();
            let kind = object
                .and_then(|object| object.get("type"))
                .and_then(JsonValue::as_str);
            match (kind, object) {
                (Some("text"), Some(object)) => ToolResultContentPart::Text {
                    text: object
                        .get("text")
                        .and_then(JsonValue::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    provider_options: None,
                },
                (Some("image"), Some(object)) => {
                    base64_file(object, "image/png").unwrap_or_else(|| json_text(item))
                }
                (Some("audio"), Some(object)) => {
                    base64_file(object, "audio/wav").unwrap_or_else(|| json_text(item))
                }
                _ => json_text(item),
            }
        })
        .collect();
    ToolResultOutput::Content { value: parts }
}

fn json_text(item: &JsonValue) -> ToolResultContentPart {
    ToolResultContentPart::Text {
        text: item.to_string(),
        provider_options: None,
    }
}

/// `inputSchema` normalised for dynamic tools: `properties` always present,
/// `additionalProperties: false`.
fn automatic_input_schema(input_schema: &JsonObject) -> JsonValue {
    let mut schema = input_schema.clone();
    schema
        .entry("properties")
        .or_insert_with(|| JsonValue::Object(JsonObject::new()));
    schema.insert("additionalProperties".to_owned(), JsonValue::Bool(false));
    JsonValue::Object(schema)
}

impl McpClient {
    /// Lists the server's tools and converts them into a [`ToolSet`].
    ///
    /// # Errors
    ///
    /// Returns the `tools/list` failure or [`McpError::InvalidArgument`] for
    /// duplicate tool names.
    pub async fn tools(&self, options: ToolsOptions) -> Result<ToolSet, McpError> {
        let request_options = RequestOptions {
            timeout: options.request_timeout,
            ..RequestOptions::default()
        };
        let definitions = self.list_all_tools(request_options).await?;
        self.tools_from_definitions(definitions, &options)
    }

    /// Converts already listed tool definitions into a [`ToolSet`].
    ///
    /// Tools whose `x-mcp-header` bindings are invalid are dropped and
    /// reported through the uncaught-error hook.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::InvalidArgument`] for duplicate tool names.
    pub fn tools_from_definitions(
        &self,
        definitions: Vec<McpTool>,
        options: &ToolsOptions,
    ) -> Result<ToolSet, McpError> {
        let bind_headers = self.protocol_era().is_modern()
            && self
                .inner
                .transport
                .capabilities()
                .supports_tool_parameter_headers;
        let mut set = ToolSet::new();
        for definition in definitions {
            let schemas = match &options.schemas {
                ToolSchemas::Automatic => None,
                ToolSchemas::Explicit(schemas) => match schemas.get(&definition.name) {
                    Some(pair) => Some(pair.clone()),
                    None => continue,
                },
                #[allow(unreachable_patterns, reason = "ToolSchemas is non-exhaustive")]
                _ => None,
            };
            let bindings = if bind_headers {
                match header_bindings(&JsonValue::Object(definition.input_schema.clone())) {
                    Ok(bindings) => bindings,
                    Err(error) => {
                        self.inner.report(McpError::invalid_argument(format!(
                            "tool {} dropped: {error}",
                            definition.name
                        )));
                        continue;
                    }
                }
            } else {
                Vec::new()
            };
            let tool = self.build_tool(&definition, schemas, bindings, options)?;
            let name = match &options.name_prefix {
                Some(prefix) => format!("{prefix}{}", definition.name),
                None => definition.name.clone(),
            };
            set.try_insert(name.as_str(), tool).map_err(|_| {
                McpError::invalid_argument(format!("duplicate MCP tool name {name}"))
            })?;
        }
        Ok(set)
    }

    fn build_tool(
        &self,
        definition: &McpTool,
        schemas: Option<ToolSchemaPair>,
        bindings: Vec<HeaderBinding>,
        options: &ToolsOptions,
    ) -> Result<Tool, McpError> {
        let app = app_tool_meta(definition)?;
        let mut metadata = JsonObject::new();
        metadata.insert(
            "clientName".to_owned(),
            JsonValue::from(self.inner.config.name.as_str()),
        );
        metadata.insert(
            "toolName".to_owned(),
            JsonValue::from(definition.name.as_str()),
        );
        if let Some(title) = &definition.title {
            metadata.insert("title".to_owned(), JsonValue::from(title.as_str()));
        }
        if let Some(annotations) = &definition.annotations {
            metadata.insert(
                "annotations".to_owned(),
                serde_json::to_value(annotations).unwrap_or_else(|_| json!({})),
            );
        }
        if let Some(app) = &app {
            metadata.insert(
                "app".to_owned(),
                serde_json::to_value(app).unwrap_or_else(|_| json!({})),
            );
        }
        if let Some(meta) = &definition.meta {
            metadata.insert("meta".to_owned(), JsonValue::Object(meta.clone()));
        }
        let (mut builder, output_schema) = match schemas {
            Some(pair) => (Tool::function_with_schema(pair.input), pair.output),
            None => (
                // Server-provided schemas rarely satisfy strict-mode rules
                // (every property required, `additionalProperties: false`),
                // so providers must not enforce them strictly.
                Tool::dynamic(Schema::from_json_schema(automatic_input_schema(
                    &definition.input_schema,
                )))
                .strict(false),
                None,
            ),
        };
        if let Some(description) = &definition.description {
            builder = builder.description(description.clone());
        }
        if let Some(title) = definition.title.as_deref().or(definition
            .annotations
            .as_ref()
            .and_then(|a| a.title.as_deref()))
        {
            builder = builder.title(title);
        }
        if let Some(schema) = &output_schema {
            builder = builder.output_schema(schema.clone());
        }
        let executor = McpToolExecutor {
            client: self.clone(),
            tool_name: definition.name.clone(),
            request_timeout: options.request_timeout,
            header_bindings: bindings.into(),
            output_schema,
        };
        Ok(builder
            .metadata(metadata)
            .execute_with(Arc::new(executor))
            .to_model_output(mcp_to_model_output)
            .build())
    }
}
