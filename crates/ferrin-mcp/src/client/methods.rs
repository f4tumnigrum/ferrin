//! Typed MCP methods: tools, resources, prompts, completion, logging.

use std::collections::BTreeMap;

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;

use super::McpClient;
use super::request::RequestOptions;
use super::request::parse_result;
use crate::error::McpError;
use crate::protocol::CallToolResult;
use crate::protocol::CompleteParams;
use crate::protocol::CompleteResult;
use crate::protocol::GetPromptResult;
use crate::protocol::ListPromptsResult;
use crate::protocol::ListResourceTemplatesResult;
use crate::protocol::ListResourcesResult;
use crate::protocol::ListToolsResult;
use crate::protocol::McpTool;
use crate::protocol::ReadResourceResult;

fn cursor_params(cursor: Option<&str>) -> Option<JsonObject> {
    cursor.map(|cursor| {
        let mut params = JsonObject::new();
        params.insert("cursor".to_owned(), JsonValue::from(cursor));
        params
    })
}

impl McpClient {
    /// Lists one page of tools.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::UnsupportedCapability`] when the server has no
    /// `tools` capability, or the request failure.
    pub async fn list_tools(
        &self,
        cursor: Option<&str>,
        options: RequestOptions,
    ) -> Result<ListToolsResult, McpError> {
        let result = self
            .inner
            .request("tools/list", cursor_params(cursor), &options)
            .await?;
        parse_result("tools/list", result)
    }

    /// Lists every tool, following pagination.
    ///
    /// # Errors
    ///
    /// See [`McpClient::list_tools`].
    pub async fn list_all_tools(&self, options: RequestOptions) -> Result<Vec<McpTool>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self.list_tools(cursor.as_deref(), options.clone()).await?;
            tools.extend(page.tools);
            match page.next_cursor {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => return Ok(tools),
            }
        }
    }

    /// Calls a tool, retrying transport failures up to the configured
    /// `max_tool_call_retries`.
    ///
    /// # Errors
    ///
    /// Returns the request failure; JSON-RPC errors are never retried.
    #[tracing::instrument(skip_all, fields(tool = %name))]
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<JsonObject>,
        options: RequestOptions,
    ) -> Result<CallToolResult, McpError> {
        let mut params = JsonObject::new();
        params.insert("name".to_owned(), JsonValue::from(name));
        params.insert(
            "arguments".to_owned(),
            JsonValue::Object(arguments.unwrap_or_default()),
        );
        let mut attempt = 0;
        loop {
            match self
                .inner
                .request("tools/call", Some(params.clone()), &options)
                .await
            {
                Ok(result) => return parse_result("tools/call", result),
                Err(error)
                    if attempt < self.inner.config.max_tool_call_retries
                        && error.is_retryable_tool_call() =>
                {
                    attempt += 1;
                    tracing::debug!(attempt, error = %error, "retrying tools/call");
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Lists one page of resources.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::UnsupportedCapability`] when the server has no
    /// `resources` capability, or the request failure.
    pub async fn list_resources(
        &self,
        cursor: Option<&str>,
        options: RequestOptions,
    ) -> Result<ListResourcesResult, McpError> {
        let result = self
            .inner
            .request("resources/list", cursor_params(cursor), &options)
            .await?;
        parse_result("resources/list", result)
    }

    /// Lists one page of resource templates.
    ///
    /// # Errors
    ///
    /// See [`McpClient::list_resources`].
    pub async fn list_resource_templates(
        &self,
        cursor: Option<&str>,
        options: RequestOptions,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let result = self
            .inner
            .request("resources/templates/list", cursor_params(cursor), &options)
            .await?;
        parse_result("resources/templates/list", result)
    }

    /// Reads a resource.
    ///
    /// # Errors
    ///
    /// See [`McpClient::list_resources`].
    pub async fn read_resource(
        &self,
        uri: &str,
        options: RequestOptions,
    ) -> Result<ReadResourceResult, McpError> {
        let mut params = JsonObject::new();
        params.insert("uri".to_owned(), JsonValue::from(uri));
        let result = self
            .inner
            .request("resources/read", Some(params), &options)
            .await?;
        parse_result("resources/read", result)
    }

    /// Lists one page of prompts.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::UnsupportedCapability`] when the server has no
    /// `prompts` capability, or the request failure.
    pub async fn list_prompts(
        &self,
        cursor: Option<&str>,
        options: RequestOptions,
    ) -> Result<ListPromptsResult, McpError> {
        let result = self
            .inner
            .request("prompts/list", cursor_params(cursor), &options)
            .await?;
        parse_result("prompts/list", result)
    }

    /// Gets a prompt with `arguments`.
    ///
    /// # Errors
    ///
    /// See [`McpClient::list_prompts`].
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<BTreeMap<String, String>>,
        options: RequestOptions,
    ) -> Result<GetPromptResult, McpError> {
        let mut params = JsonObject::new();
        params.insert("name".to_owned(), JsonValue::from(name));
        if let Some(arguments) = arguments {
            params.insert(
                "arguments".to_owned(),
                serde_json::to_value(arguments).unwrap_or(JsonValue::Null),
            );
        }
        let result = self
            .inner
            .request("prompts/get", Some(params), &options)
            .await?;
        parse_result("prompts/get", result)
    }

    /// Requests argument completions.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::UnsupportedCapability`] when the server has no
    /// `completions` capability, or the request failure.
    pub async fn complete(
        &self,
        params: CompleteParams,
        options: RequestOptions,
    ) -> Result<CompleteResult, McpError> {
        let params = match serde_json::to_value(params) {
            Ok(JsonValue::Object(object)) => object,
            _ => {
                return Err(McpError::invalid_argument(
                    "completion params must serialize to an object",
                ));
            }
        };
        let result = self
            .inner
            .request("completion/complete", Some(params), &options)
            .await?;
        parse_result("completion/complete", result)
    }

    /// Pings the server.
    ///
    /// # Errors
    ///
    /// Returns the request failure.
    pub async fn ping(&self, options: RequestOptions) -> Result<(), McpError> {
        self.inner.request("ping", None, &options).await.map(|_| ())
    }

    /// Sets the server's logging level (`logging/setLevel`).
    ///
    /// # Errors
    ///
    /// Returns the request failure.
    pub async fn set_logging_level(
        &self,
        level: &str,
        options: RequestOptions,
    ) -> Result<(), McpError> {
        let mut params = JsonObject::new();
        params.insert("level".to_owned(), JsonValue::from(level));
        self.inner
            .request("logging/setLevel", Some(params), &options)
            .await
            .map(|_| ())
    }
}
