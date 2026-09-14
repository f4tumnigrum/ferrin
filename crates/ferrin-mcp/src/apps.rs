//! MCP Apps: tools that render an interactive `ui://` resource.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_tool::fingerprint::hash_canonical;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

use crate::client::McpClient;
use crate::client::RequestOptions;
use crate::error::McpError;
use crate::protocol::ClientCapabilities;
use crate::protocol::McpTool;
use crate::protocol::ReadResourceResult;

/// Extension name announced in `capabilities.extensions`.
pub const MCP_APP_EXTENSION_NAME: &str = "io.modelcontextprotocol/ui";

/// MIME type of an app resource.
pub const MCP_APP_MIME_TYPE: &str = "text/html;profile=mcp-app";

/// Legacy `_meta` key carrying the resource URI.
pub const MCP_APP_LEGACY_RESOURCE_URI_META_KEY: &str = "ui/resourceUri";

/// URI scheme of app resources.
pub const MCP_APP_URI_SCHEME: &str = "ui://";

/// Client capabilities declaring MCP Apps support.
#[must_use]
pub fn mcp_app_client_capabilities() -> ClientCapabilities {
    let mut extra = JsonObject::new();
    extra.insert(
        "extensions".to_owned(),
        json!({ MCP_APP_EXTENSION_NAME: { "mimeTypes": [MCP_APP_MIME_TYPE] } }),
    );
    ClientCapabilities {
        elicitation: None,
        extra,
    }
}

/// App metadata of a tool (`_meta.ui`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppToolMeta {
    /// URI of the resource rendering the tool (`ui://...`).
    pub resource_uri: String,
    /// Who sees the tool: `model`, `app` or both (default both).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Vec<String>>,
    /// Further fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

impl McpAppToolMeta {
    /// Whether the model may call the tool.
    #[must_use]
    pub fn is_model_visible(&self) -> bool {
        self.visibility
            .as_ref()
            .is_none_or(|visibility| visibility.iter().any(|entry| entry == "model"))
    }

    /// Whether the app may call the tool.
    #[must_use]
    pub fn is_app_visible(&self) -> bool {
        self.visibility
            .as_ref()
            .is_none_or(|visibility| visibility.iter().any(|entry| entry == "app"))
    }
}

/// Reads the app metadata of `tool`.
///
/// # Errors
///
/// Returns [`McpError::InvalidArgument`] when the metadata is malformed or
/// the resource URI does not use the `ui://` scheme.
pub fn app_tool_meta(tool: &McpTool) -> Result<Option<McpAppToolMeta>, McpError> {
    let Some(meta) = &tool.meta else {
        return Ok(None);
    };
    let parsed = match meta.get("ui") {
        Some(ui @ JsonValue::Object(_)) => serde_json::from_value::<McpAppToolMeta>(ui.clone())
            .map_err(|error| {
                McpError::invalid_argument(format!(
                    "tool {} has invalid _meta.ui: {error}",
                    tool.name
                ))
            })?,
        Some(_) => {
            return Err(McpError::invalid_argument(format!(
                "tool {} has a non-object _meta.ui",
                tool.name
            )));
        }
        None => match meta.get(MCP_APP_LEGACY_RESOURCE_URI_META_KEY) {
            Some(JsonValue::String(uri)) => McpAppToolMeta {
                resource_uri: uri.clone(),
                visibility: None,
                extra: JsonObject::new(),
            },
            Some(_) => {
                return Err(McpError::invalid_argument(format!(
                    "tool {} has a non-string {MCP_APP_LEGACY_RESOURCE_URI_META_KEY}",
                    tool.name
                )));
            }
            None => return Ok(None),
        },
    };
    if !parsed.resource_uri.starts_with(MCP_APP_URI_SCHEME) {
        return Err(McpError::invalid_argument(format!(
            "tool {} app resource uri must start with {MCP_APP_URI_SCHEME}",
            tool.name
        )));
    }
    Ok(Some(parsed))
}

/// The app resource URI of `tool`, if it is an app tool.
///
/// # Errors
///
/// See [`app_tool_meta`].
pub fn app_resource_uri(tool: &McpTool) -> Result<Option<String>, McpError> {
    Ok(app_tool_meta(tool)?.map(|meta| meta.resource_uri))
}

/// Whether `tool` carries app metadata.
#[must_use]
pub fn is_app_tool(tool: &McpTool) -> bool {
    matches!(app_tool_meta(tool), Ok(Some(_)))
}

/// Tools split by audience.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplitAppTools {
    /// Tools the model may call (ordinary tools and model-visible app tools).
    pub model_tools: Vec<McpTool>,
    /// App tools the app may call.
    pub app_tools: Vec<McpTool>,
}

/// Splits `tools` into model-visible and app-visible tools.
///
/// # Errors
///
/// See [`app_tool_meta`].
pub fn split_app_tools(tools: Vec<McpTool>) -> Result<SplitAppTools, McpError> {
    let mut split = SplitAppTools::default();
    for tool in tools {
        match app_tool_meta(&tool)? {
            None => split.model_tools.push(tool),
            Some(meta) => {
                if meta.is_model_visible() {
                    split.model_tools.push(tool.clone());
                }
                if meta.is_app_visible() {
                    split.app_tools.push(tool);
                }
            }
        }
    }
    Ok(split)
}

/// Distinct app resource URIs referenced by `tools`, in first-seen order.
///
/// # Errors
///
/// See [`app_tool_meta`].
pub fn app_resource_uris(tools: &[McpTool]) -> Result<Vec<String>, McpError> {
    let mut uris: Vec<String> = Vec::new();
    for tool in tools {
        if let Some(uri) = app_resource_uri(tool)?
            && !uris.contains(&uri)
        {
            uris.push(uri);
        }
    }
    Ok(uris)
}

/// Content security policy of an app resource.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppResourceCsp {
    /// Origins the app may connect to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_domains: Option<Vec<String>>,
    /// Origins the app may load resources from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_domains: Option<Vec<String>>,
    /// Origins the app may frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_domains: Option<Vec<String>>,
    /// Further fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// `_meta.ui` of an app resource.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppResourceMeta {
    /// Whether the host should draw a border.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefers_border: Option<bool>,
    /// Content security policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csp: Option<McpAppResourceCsp>,
    /// Requested permissions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<JsonValue>,
    /// Further fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// An app resource read from the server.
#[derive(Debug, Clone, PartialEq)]
pub struct McpAppResource {
    /// Resource URI.
    pub uri: String,
    /// MIME type as sent by the server.
    pub mime_type: String,
    /// HTML document.
    pub html: String,
    /// `_meta.ui` of the resource.
    pub meta: Option<McpAppResourceMeta>,
}

fn is_app_mime_type(mime_type: &str) -> bool {
    let normalized: String = mime_type
        .split(';')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(";")
        .to_ascii_lowercase();
    normalized == MCP_APP_MIME_TYPE
}

/// Extracts the app resource `uri` from a `resources/read` result.
///
/// # Errors
///
/// Returns [`McpError::Protocol`] when the result has no text content of
/// type [`MCP_APP_MIME_TYPE`] or its `_meta.ui` is malformed.
pub fn app_resource_from_read_result(
    uri: &str,
    result: &ReadResourceResult,
) -> Result<McpAppResource, McpError> {
    let content = result
        .contents
        .iter()
        .find(|content| {
            content.mime_type.as_deref().is_some_and(is_app_mime_type) && content.text.is_some()
        })
        .ok_or_else(|| {
            McpError::protocol(format!(
                "resource {uri} has no {MCP_APP_MIME_TYPE} text content"
            ))
        })?;
    let meta = match content.meta.as_ref().and_then(|meta| meta.get("ui")) {
        Some(ui) => Some(serde_json::from_value(ui.clone()).map_err(|error| {
            McpError::protocol(format!("resource {uri} has invalid _meta.ui: {error}"))
        })?),
        None => None,
    };
    Ok(McpAppResource {
        uri: content.uri.clone(),
        mime_type: content.mime_type.clone().unwrap_or_default(),
        html: content.text.clone().unwrap_or_default(),
        meta,
    })
}

/// Reads the app resource `uri` through `client`.
///
/// # Errors
///
/// Returns the `resources/read` failure or the extraction failure of
/// [`app_resource_from_read_result`].
pub async fn read_app_resource(
    client: &McpClient,
    uri: &str,
    options: RequestOptions,
) -> Result<McpAppResource, McpError> {
    let result = client.read_resource(uri, options).await?;
    app_resource_from_read_result(uri, &result)
}

/// SHA-256 (base64url) fingerprint of the HTML, CSP and permissions of an
/// app resource.
#[must_use]
pub fn fingerprint_app_resource(resource: &McpAppResource) -> String {
    let meta = resource.meta.as_ref();
    hash_canonical(&json!({
        "html": resource.html,
        "csp": meta.and_then(|meta| meta.csp.as_ref()),
        "permissions": meta.and_then(|meta| meta.permissions.as_ref()),
    }))
}

/// Whether `current` differs from `baseline` in HTML, CSP or permissions.
#[must_use]
pub fn detect_app_resource_drift(current: &McpAppResource, baseline: &McpAppResource) -> bool {
    fingerprint_app_resource(current) != fingerprint_app_resource(baseline)
}
