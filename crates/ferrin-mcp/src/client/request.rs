//! Request execution: `_meta` injection, timeouts, `input_required`
//! rounds and protocol negotiation.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::ClientInner;
use super::McpClient;
use crate::error::McpError;
use crate::protocol::DiscoverResult;
use crate::protocol::InitializeResult;
use crate::protocol::JsonRpcMessage;
use crate::protocol::LATEST_LEGACY_PROTOCOL_VERSION;
use crate::protocol::LATEST_PROTOCOL_VERSION;
use crate::protocol::META_CLIENT_CAPABILITIES;
use crate::protocol::META_CLIENT_INFO;
use crate::protocol::META_PROTOCOL_VERSION;
use crate::protocol::META_SERVER_INFO;
use crate::protocol::MODERN_PROTOCOL_ERROR_CODES;
use crate::protocol::ProtocolEra;
use crate::protocol::is_supported_version;
use crate::transport::SendOptions;
use crate::transport::lock;

/// Per-request options.
#[derive(Debug, Clone, Default)]
pub struct RequestOptions {
    /// Timeout of this request (overrides the client default).
    pub timeout: Option<Duration>,
    /// Upper bound applied after `timeout`.
    pub max_total_timeout: Option<Duration>,
    /// Cancels the request and its transport operation.
    pub cancellation: Option<CancellationToken>,
    /// Request-specific HTTP headers (`Mcp-Param-*`), used by HTTP transports.
    pub headers: Headers,
}

impl RequestOptions {
    /// Options with only a timeout.
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout: Some(timeout),
            ..Self::default()
        }
    }

    /// Sets the cancellation token.
    #[must_use]
    pub fn cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    fn effective_timeout(&self, default: Option<Duration>) -> Option<Duration> {
        match (self.timeout.or(default), self.max_total_timeout) {
            (Some(timeout), Some(max)) => Some(timeout.min(max)),
            (Some(timeout), None) => Some(timeout),
            (None, max) => max,
        }
    }
}

/// Deserializes a result object into `T`.
pub(crate) fn parse_result<T: DeserializeOwned>(
    method: &str,
    result: JsonObject,
) -> Result<T, McpError> {
    serde_json::from_value(JsonValue::Object(result))
        .map_err(|error| McpError::protocol(format!("invalid {method} result: {error}")))
}

/// Capability a method family requires.
fn required_capability(method: &str) -> Option<&'static str> {
    if method == "completion/complete" {
        Some("completions")
    } else if method.starts_with("tools/") {
        Some("tools")
    } else if method.starts_with("resources/") {
        Some("resources")
    } else if method.starts_with("prompts/") {
        Some("prompts")
    } else {
        None
    }
}

/// Remove a pending registration even when the calling future is dropped.
struct PendingRequest<'a> {
    inner: &'a ClientInner,
    id: i64,
}

impl Drop for PendingRequest<'_> {
    fn drop(&mut self) {
        self.inner.unregister(self.id);
    }
}

impl ClientInner {
    fn assert_capability(&self, method: &str) -> Result<(), McpError> {
        let Some(required) = required_capability(method) else {
            return Ok(());
        };
        let state = lock(&self.state);
        let Some(capabilities) = &state.capabilities else {
            return Ok(());
        };
        let declared = match required {
            "completions" => capabilities.completions.is_some(),
            "tools" => capabilities.tools.is_some(),
            "resources" => capabilities.resources.is_some(),
            "prompts" => capabilities.prompts.is_some(),
            _ => true,
        };
        if declared {
            Ok(())
        } else {
            Err(McpError::UnsupportedCapability(format!(
                "{required} (method {method})"
            )))
        }
    }

    fn inject_meta(&self, params: &mut JsonObject, version: &str) {
        let meta = params
            .entry("_meta")
            .or_insert_with(|| JsonValue::Object(JsonObject::new()));
        if !meta.is_object() {
            *meta = JsonValue::Object(JsonObject::new());
        }
        if let Some(meta) = meta.as_object_mut() {
            meta.insert(META_PROTOCOL_VERSION.to_owned(), JsonValue::from(version));
            meta.insert(
                META_CLIENT_CAPABILITIES.to_owned(),
                serde_json::to_value(&self.config.capabilities).unwrap_or_else(|_| json!({})),
            );
            meta.insert(
                META_CLIENT_INFO.to_owned(),
                serde_json::to_value(self.client_info()).unwrap_or_else(|_| json!({})),
            );
        }
    }

    fn cancel_request(&self, id: i64, reason: &str) {
        let mut params = JsonObject::new();
        params.insert("requestId".to_owned(), JsonValue::from(id));
        params.insert("reason".to_owned(), JsonValue::from(reason));
        let message = JsonRpcMessage::notification("notifications/cancelled", Some(params));
        let Some(owner) = self.cleanup_tasks.upgrade() else {
            return;
        };
        let transport = std::sync::Arc::clone(&self.transport);
        let cancellation = self.cancellation.child_token();
        let mut tasks = lock(&owner);
        while tasks.try_join_next().is_some() {}
        tasks.spawn(async move {
            let _cancel_on_drop = cancellation.clone().drop_guard();
            let options = SendOptions { cancellation: Some(cancellation.clone()), ..SendOptions::default() };
            // Cleanup has its own bound and never extends the request deadline.
            tokio::select! {
                () = cancellation.cancelled() => {},
                _ = tokio::time::timeout(Duration::from_secs(1), transport.send(message, options)) => {},
            }
        });
    }

    /// Sends one request and waits for its raw result.
    async fn exchange(
        &self,
        method: &str,
        params: JsonObject,
        options: &RequestOptions,
        active_id: &AtomicI64,
    ) -> Result<JsonObject, McpError> {
        if self.is_closed() {
            return Err(McpError::Closed);
        }
        let id = self.next_id();
        let receiver = self.register(id);
        let _pending = PendingRequest { inner: self, id };
        active_id.store(id, Ordering::Relaxed);
        let message = JsonRpcMessage::request(id, method, Some(params));
        let send_options = SendOptions {
            cancellation: options.cancellation.clone(),
            headers: options.headers.clone(),
        };
        self.transport.send(message, send_options).await?;
        receiver.await.unwrap_or(Err(McpError::Closed))
    }

    /// Answers the `inputRequests` of an `input_required` result.
    async fn answer_input_requests(&self, requests: &JsonObject) -> Result<JsonObject, McpError> {
        let handler = lock(&self.elicitation).clone();
        let mut responses = JsonObject::new();
        for (key, request) in requests {
            let method = request
                .get("method")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            if method != "elicitation/create" {
                return Err(McpError::protocol(format!(
                    "unsupported input request method \"{method}\" (key {key})"
                )));
            }
            let Some(handler) = &handler else {
                return Err(McpError::elicitation(
                    "server requires input but no elicitation handler is registered",
                ));
            };
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let elicitation = serde_json::from_value(params).map_err(|error| {
                McpError::protocol(format!("invalid elicitation request {key}: {error}"))
            })?;
            let result = handler.handle(elicitation).await?;
            responses.insert(
                key.clone(),
                serde_json::to_value(result).unwrap_or_else(|_| json!({"action": "cancel"})),
            );
        }
        Ok(responses)
    }

    /// Applies one deadline to sending, receiving and all input rounds.
    pub(crate) async fn request(
        &self,
        method: &str,
        params: Option<JsonObject>,
        options: &RequestOptions,
    ) -> Result<JsonObject, McpError> {
        let timeout = options.effective_timeout(self.config.default_request_timeout);
        let cancellation = options
            .cancellation
            .as_ref()
            .map_or_else(CancellationToken::new, CancellationToken::child_token);
        let _cancel_on_drop = cancellation.clone().drop_guard();
        let options = RequestOptions {
            cancellation: Some(cancellation.clone()),
            ..options.clone()
        };
        let active_id = AtomicI64::new(0);
        let outcome = {
            let response = async {
                let request = self.request_rounds(method, params, &options, &active_id);
                match timeout {
                    Some(timeout) => tokio::time::timeout(timeout, request)
                        .await
                        .map_err(|_| McpError::Timeout(timeout))?,
                    None => request.await,
                }
            };
            tokio::select! {
                biased;
                () = self.cancellation.cancelled() => Err(McpError::Closed),
                () = cancellation.cancelled() => Err(McpError::Cancelled),
                outcome = response => outcome,
            }
        };
        cancellation.cancel();
        let reason = match &outcome {
            Err(McpError::Timeout(_)) => Some("timeout"),
            Err(McpError::Cancelled) => Some("cancelled"),
            _ => None,
        };
        if let Some(reason) = reason
            && self.config.send_cancel_notifications
        {
            let id = active_id.load(Ordering::Relaxed);
            if id != 0 {
                self.cancel_request(id, reason);
            }
        }
        outcome
    }

    /// Sends a request, following `input_required` rounds in the modern era.
    async fn request_rounds(
        &self,
        method: &str,
        params: Option<JsonObject>,
        options: &RequestOptions,
        active_id: &AtomicI64,
    ) -> Result<JsonObject, McpError> {
        self.assert_capability(method)?;
        let mut params = params.unwrap_or_default();
        let (era, version) = {
            let state = lock(&self.state);
            (state.era(), state.protocol_version.clone())
        };
        if let (ProtocolEra::Modern, Some(version)) = (era, &version) {
            self.inject_meta(&mut params, version);
        }
        let mut rounds = 0;
        loop {
            let result = self
                .exchange(method, params.clone(), options, active_id)
                .await?;
            if result.get("resultType").and_then(JsonValue::as_str) == Some("input_required")
                && self.config.max_input_rounds == 0
            {
                return Err(McpError::protocol(
                    "server requested additional input, but multi round-trip requests are not enabled",
                ));
            }
            if era != ProtocolEra::Modern {
                return Ok(result);
            }
            match result.get("resultType").and_then(JsonValue::as_str) {
                Some("complete") => return Ok(result),
                Some("input_required") => {}
                Some(other) => {
                    return Err(McpError::protocol(format!(
                        "unknown MCP resultType \"{other}\""
                    )));
                }
                None => {
                    return Err(McpError::protocol(
                        "modern MCP result is missing resultType",
                    ));
                }
            }
            rounds += 1;
            if rounds > self.config.max_input_rounds {
                return Err(McpError::protocol(format!(
                    "request {method} required input more than {} times",
                    self.config.max_input_rounds
                )));
            }
            let requests = result
                .get("inputRequests")
                .and_then(JsonValue::as_object)
                .cloned()
                .unwrap_or_default();
            let responses = self.answer_input_requests(&requests).await?;
            params.insert("inputResponses".to_owned(), JsonValue::Object(responses));
            match result.get("requestState") {
                Some(state) => {
                    params.insert("requestState".to_owned(), state.clone());
                }
                None => {
                    params.remove("requestState");
                }
            }
        }
    }

    pub(crate) async fn notify(
        &self,
        method: &str,
        params: Option<JsonObject>,
    ) -> Result<(), McpError> {
        if self.is_closed() {
            return Err(McpError::Closed);
        }
        let message = JsonRpcMessage::notification(method, params);
        self.transport.send(message, SendOptions::default()).await
    }

    fn set_negotiated(
        &self,
        version: &str,
        result_capabilities: crate::protocol::ServerCapabilities,
        info: Option<crate::protocol::Implementation>,
        instructions: Option<String>,
    ) {
        {
            let mut state = lock(&self.state);
            state.protocol_version = Some(version.to_owned());
            state.capabilities = Some(result_capabilities);
            state.info = info;
            state.instructions = instructions;
        }
        self.transport.set_protocol_version(Some(version));
    }

    /// Probes `server/discover`; `Ok(None)` means the server does not speak
    /// the modern protocol and the legacy flow should run.
    async fn discover(&self) -> Result<Option<()>, McpError> {
        self.transport
            .set_protocol_version(Some(LATEST_PROTOCOL_VERSION));
        lock(&self.state).protocol_version = Some(LATEST_PROTOCOL_VERSION.to_owned());
        let options = RequestOptions::with_timeout(self.config.discovery_timeout);
        let outcome = async {
            let result = self.request("server/discover", None, &options).await?;
            let discovered: DiscoverResult = parse_result("server/discover", result)?;
            if !discovered.supported_versions.iter().any(|version| version == LATEST_PROTOCOL_VERSION) {
                return Err(McpError::protocol(format!(
                    "server does not support the requested protocol version: {LATEST_PROTOCOL_VERSION}"
                )));
            }
            Ok(discovered)
        }.await;
        let discovered = match outcome {
            Ok(result) => result,
            Err(error) => {
                let modern = error
                    .code()
                    .is_some_and(|code| MODERN_PROTOCOL_ERROR_CODES.contains(&code));
                if modern {
                    return Err(error);
                }
                tracing::debug!(error = %error, "server/discover failed; falling back to initialize");
                self.transport.set_protocol_version(None);
                lock(&self.state).protocol_version = None;
                return Ok(None);
            }
        };
        let info = discovered
            .meta
            .as_ref()
            .and_then(|meta| meta.get(META_SERVER_INFO))
            .and_then(|value| serde_json::from_value(value.clone()).ok());
        self.set_negotiated(
            LATEST_PROTOCOL_VERSION,
            discovered.capabilities,
            info,
            discovered.instructions,
        );
        Ok(Some(()))
    }

    async fn initialize_legacy(&self) -> Result<(), McpError> {
        let mut params = JsonObject::new();
        params.insert(
            "protocolVersion".to_owned(),
            JsonValue::from(LATEST_LEGACY_PROTOCOL_VERSION),
        );
        params.insert(
            "capabilities".to_owned(),
            serde_json::to_value(&self.config.capabilities).unwrap_or_else(|_| json!({})),
        );
        params.insert(
            "clientInfo".to_owned(),
            serde_json::to_value(self.client_info()).unwrap_or_else(|_| json!({})),
        );
        let options = RequestOptions {
            timeout: self.config.initialization_timeout,
            ..RequestOptions::default()
        };
        let result = self.request("initialize", Some(params), &options).await?;
        let initialized: InitializeResult = parse_result("initialize", result)?;
        if !is_supported_version(&initialized.protocol_version) {
            return Err(McpError::protocol(format!(
                "server protocol version {} is not supported",
                initialized.protocol_version
            )));
        }
        self.set_negotiated(
            &initialized.protocol_version,
            initialized.capabilities,
            Some(initialized.server_info),
            initialized.instructions,
        );
        self.notify("notifications/initialized", None).await
    }
}

impl McpClient {
    pub(crate) async fn initialize(&self) -> Result<(), McpError> {
        let inner = &self.inner;
        if inner.config.protocol_discovery
            && inner
                .transport
                .capabilities()
                .supports_protocol_version_discovery
            && inner.discover().await?.is_some()
        {
            return Ok(());
        }
        inner.initialize_legacy().await
    }

    /// Sends a raw request and returns the result object.
    ///
    /// Modern-era requests carry the client `_meta` and follow
    /// `input_required` rounds through the elicitation handler.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::UnsupportedCapability`] when the server did not
    /// declare the capability the method family needs, the JSON-RPC error the
    /// server replied with, or a transport, timeout or protocol failure.
    pub async fn request(
        &self,
        method: &str,
        params: Option<JsonObject>,
        options: RequestOptions,
    ) -> Result<JsonObject, McpError> {
        self.inner.request(method, params, &options).await
    }

    /// Sends a notification.
    ///
    /// # Errors
    ///
    /// Returns the transport failure.
    pub async fn notify(&self, method: &str, params: Option<JsonObject>) -> Result<(), McpError> {
        self.inner.notify(method, params).await
    }
}
