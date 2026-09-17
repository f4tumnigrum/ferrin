//! MCP Apps: tools that render an interactive `ui://` resource.

use base64::Engine;
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
    /// URI of the resource rendering the tool (`ui://...`), when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_uri: Option<String>,
    /// Who sees the tool: `model`, `app` or both (default model only).
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
            .is_some_and(|visibility| visibility.iter().any(|entry| entry == "app"))
    }
}

/// Reads the app metadata of `tool`.
///
/// # Errors
///
/// Returns [`McpError::InvalidArgument`] when a supplied resource URI is not
/// a string using the `ui://` scheme. Non-object UI metadata is ignored.
pub fn app_tool_meta(tool: &McpTool) -> Result<Option<McpAppToolMeta>, McpError> {
    let Some(meta) = &tool.meta else {
        return Ok(None);
    };
    let ui = meta.get("ui").and_then(JsonValue::as_object);
    let resource = ui
        .and_then(|ui| ui.get("resourceUri"))
        .filter(|value| !value.is_null())
        .or_else(|| meta.get(MCP_APP_LEGACY_RESOURCE_URI_META_KEY));
    let resource_uri = match resource {
        Some(JsonValue::String(uri)) if uri.starts_with(MCP_APP_URI_SCHEME) => Some(uri.clone()),
        Some(_) => {
            return Err(McpError::invalid_argument(format!(
                "tool {} app resource uri must start with {MCP_APP_URI_SCHEME}",
                tool.name
            )));
        }
        None => None,
    };
    if ui.is_none() && resource_uri.is_none() {
        return Ok(None);
    }
    let mut extra = ui.cloned().unwrap_or_default();
    extra.remove("resourceUri");
    let visibility = extra.remove("visibility").and_then(|value| {
        value.as_array().map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .filter(|value| matches!(*value, "model" | "app"))
                .map(str::to_owned)
                .collect()
        })
    });
    let parsed = McpAppToolMeta {
        resource_uri,
        visibility,
        extra,
    };
    Ok(Some(parsed))
}

/// The app resource URI of `tool`, if it is an app tool.
///
/// # Errors
///
/// See [`app_tool_meta`].
pub fn app_resource_uri(tool: &McpTool) -> Result<Option<String>, McpError> {
    Ok(app_tool_meta(tool)?.and_then(|meta| meta.resource_uri))
}

/// Whether `tool` references an app resource.
#[must_use]
pub fn is_app_tool(tool: &McpTool) -> bool {
    matches!(app_resource_uri(tool), Ok(Some(_)))
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

fn string_array(value: Option<JsonValue>) -> Option<Vec<String>> {
    value.and_then(|value| {
        value.as_array().map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_owned)
                .collect()
        })
    })
}

fn resource_meta(meta: Option<&JsonObject>) -> Option<McpAppResourceMeta> {
    let mut ui = meta?.get("ui")?.as_object()?.clone();
    let prefers_border = ui.remove("prefersBorder").and_then(|value| value.as_bool());
    let csp = ui
        .remove("csp")
        .and_then(|value| value.as_object().cloned())
        .map(|mut csp| McpAppResourceCsp {
            connect_domains: string_array(csp.remove("connectDomains")),
            resource_domains: string_array(csp.remove("resourceDomains")),
            frame_domains: string_array(csp.remove("frameDomains")),
            extra: csp,
        });
    let permissions = ui.remove("permissions").filter(JsonValue::is_object);
    Some(McpAppResourceMeta {
        prefers_border,
        csp,
        permissions,
        extra: ui,
    })
}

/// Extracts the requested app resource from a `resources/read` result.
///
/// # Errors
///
/// Returns [`McpError::Protocol`] for a missing URI, an unsupported MIME type,
/// missing HTML, or invalid base64 content. Malformed known rendering metadata
/// fields are omitted individually.
pub fn app_resource_from_read_result(
    uri: &str,
    result: &ReadResourceResult,
) -> Result<McpAppResource, McpError> {
    let content = result
        .contents
        .iter()
        .find(|content| content.uri == uri)
        .ok_or_else(|| McpError::protocol(format!("resource {uri} was not returned")))?;
    if content.mime_type.as_deref() != Some(MCP_APP_MIME_TYPE) {
        return Err(McpError::protocol(format!(
            "resource {uri} has an unsupported app mime type"
        )));
    }
    let html = match (&content.text, &content.blob) {
        (Some(text), _) => text.clone(),
        (_, Some(blob)) => {
            let data = base64::engine::general_purpose::STANDARD
                .decode(blob)
                .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(blob))
                .map_err(|_| {
                    McpError::protocol(format!("resource {uri} contains invalid base64"))
                })?;
            String::from_utf8_lossy(&data).into_owned()
        }
        _ => {
            return Err(McpError::protocol(format!(
                "resource {uri} has no app html content"
            )));
        }
    };
    Ok(McpAppResource {
        uri: uri.to_owned(),
        mime_type: MCP_APP_MIME_TYPE.to_owned(),
        html,
        meta: resource_meta(content.meta.as_ref()),
    })
}

/// Reads the app resource `uri` through `client`.
///
/// # Errors
///
/// Returns [`McpError::InvalidArgument`] for a non-`ui://` URI, the
/// `resources/read` failure or the extraction failure of
/// [`app_resource_from_read_result`].
pub async fn read_app_resource(
    client: &McpClient,
    uri: &str,
    options: RequestOptions,
) -> Result<McpAppResource, McpError> {
    if !uri.starts_with(MCP_APP_URI_SCHEME) {
        return Err(McpError::invalid_argument(
            "app resource uri must start with ui://",
        ));
    }
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
