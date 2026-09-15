//! Capability middleware: restrict the tools offered to the model.
//!
//! The pattern follows the Vercel AI SDK capability middleware (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), reimplemented for Ferrin's middleware
//! interface.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use ferrin_core::middleware::CallKind;
use ferrin_core::middleware::LanguageModelMiddleware;
use ferrin_core::middleware::MiddlewareContext;
use ferrin_spec::BoxFuture;
use ferrin_spec::CallOptions;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolChoice;
use ferrin_spec::error::ProviderError;
use serde_json::json;

use crate::approval::FailureMode;
use crate::client::PolicyClient;

/// Builds the policy input for a model call.
pub type CapabilityInputFn =
    Arc<dyn Fn(&CallOptions, &MiddlewareContext<'_>) -> JsonValue + Send + Sync>;

/// The default capability input:
///
/// ```json
/// {
///   "model": { "provider": "..", "model_id": ".." },
///   "call": "generate" | "stream",
///   "tools": [ { "name": "..", "provider_defined": false }, .. ],
///   "tool_choice": <tool choice or null>
/// }
/// ```
#[must_use]
pub fn default_capability_input(options: &CallOptions, ctx: &MiddlewareContext<'_>) -> JsonValue {
    let call = match ctx.kind {
        CallKind::Generate => "generate",
        CallKind::Stream => "stream",
        _ => "unknown",
    };
    json!({
        "model": {
            "provider": ctx.model.provider(),
            "model_id": ctx.model.model_id(),
        },
        "call": call,
        "tools": options
            .tools
            .iter()
            .map(|tool| json!({
                "name": tool.name(),
                "provider_defined": tool.is_provider_tool(),
            }))
            .collect::<Vec<_>>(),
        "tool_choice": serde_json::to_value(&options.tool_choice).unwrap_or(JsonValue::Null),
    })
}

/// Parses the allowlist of a capability decision: an array of tool names or
/// an object with a `tools` array. Returns `None` for anything else.
#[must_use]
pub fn parse_allowlist(raw: &JsonValue) -> Option<BTreeSet<String>> {
    let names = match raw {
        JsonValue::Array(names) => names,
        JsonValue::Object(object) => object.get("tools")?.as_array()?,
        _ => return None,
    };
    names
        .iter()
        .map(|name| name.as_str().map(str::to_owned))
        .collect()
}

/// Middleware created by [`capability_middleware`].
pub struct CapabilityMiddleware<C> {
    client: C,
    path: String,
    to_input: Option<CapabilityInputFn>,
    on_error: FailureMode,
}

/// Restricts `CallOptions::tools` to the allowlist returned by the policy at
/// `path`.
///
/// Calls without tools are not evaluated. When the policy cannot be
/// evaluated or returns an unrecognized document, all tools are removed
/// (fail closed) unless [`CapabilityMiddleware::on_error`] selects
/// [`FailureMode::FallThrough`], which keeps the tools unchanged. A
/// `tool_choice` that forces a removed tool, or requires a tool when none
/// remains, is cleared.
pub fn capability_middleware<C: PolicyClient>(
    client: C,
    path: impl Into<String>,
) -> CapabilityMiddleware<C> {
    CapabilityMiddleware {
        client,
        path: path.into(),
        to_input: None,
        on_error: FailureMode::Deny,
    }
}

impl<C> CapabilityMiddleware<C> {
    /// Replaces the default input document.
    #[must_use]
    pub fn to_input(
        mut self,
        f: impl Fn(&CallOptions, &MiddlewareContext<'_>) -> JsonValue + Send + Sync + 'static,
    ) -> Self {
        self.to_input = Some(Arc::new(f));
        self
    }

    /// Sets the behaviour on evaluation errors (default: remove all tools).
    #[must_use]
    pub fn on_error(mut self, mode: FailureMode) -> Self {
        self.on_error = mode;
        self
    }

    /// The policy path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl<C> fmt::Debug for CapabilityMiddleware<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapabilityMiddleware")
            .field("path", &self.path)
            .field("custom_input", &self.to_input.is_some())
            .field("on_error", &self.on_error)
            .finish_non_exhaustive()
    }
}

impl<C: PolicyClient> LanguageModelMiddleware for CapabilityMiddleware<C> {
    fn transform_params<'a>(
        &'a self,
        mut options: CallOptions,
        ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<CallOptions, ProviderError>> {
        Box::pin(async move {
            if options.tools.is_empty() {
                return Ok(options);
            }
            let input = match &self.to_input {
                Some(to_input) => to_input(&options, &ctx),
                None => default_capability_input(&options, &ctx),
            };
            match self.client.evaluate(&self.path, input).await {
                Ok(raw) => match parse_allowlist(&raw) {
                    Some(allowed) => {
                        options
                            .tools
                            .retain(|tool| allowed.contains(tool.name().as_str()));
                    }
                    None => {
                        tracing::warn!(path = %self.path, "unrecognized capability decision");
                        self.fail(&mut options);
                    }
                },
                Err(_error) => {
                    tracing::warn!(path = %self.path, "capability evaluation failed");
                    self.fail(&mut options);
                }
            }
            clear_stale_tool_choice(&mut options);
            Ok(options)
        })
    }
}

impl<C> CapabilityMiddleware<C> {
    fn fail(&self, options: &mut CallOptions) {
        match self.on_error {
            FailureMode::Deny => options.tools.clear(),
            FailureMode::FallThrough => {}
        }
    }
}

fn clear_stale_tool_choice(options: &mut CallOptions) {
    let stale = match &options.tool_choice {
        Some(ToolChoice::Tool { tool_name }) => {
            !options.tools.iter().any(|tool| tool.name() == tool_name)
        }
        Some(ToolChoice::Required) => options.tools.is_empty(),
        Some(ToolChoice::Auto | ToolChoice::None) | None => false,
    };
    if stale {
        options.tool_choice = None;
    }
}
