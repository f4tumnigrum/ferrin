//! Approval policies backed by a policy client.

use std::fmt;
use std::sync::Arc;

use ferrin_core::generate_text::ApprovalContext;
use ferrin_core::generate_text::ApprovalPolicy;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_core::generate_text::ParsedToolCall;
use ferrin_spec::BoxFuture;
use ferrin_spec::JsonValue;
use serde_json::json;

use crate::client::PolicyClient;
use crate::decision::PolicyDecision;

/// What an approval policy or capability middleware does when the policy
/// cannot be evaluated (transport failure, engine error, invalid response).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FailureMode {
    /// Fail closed: deny the call (or clear the tools).
    #[default]
    Deny,
    /// Fail open: fall through to the tool's own `needs_approval` (or keep
    /// the tools unchanged).
    FallThrough,
}

/// Builds the policy input for a tool call.
pub type ToInputFn = Arc<dyn Fn(&ParsedToolCall, &ApprovalContext<'_>) -> JsonValue + Send + Sync>;

/// The default policy input:
///
/// ```json
/// {
///   "tool": { "name": "..", "tool_call_id": "..", "dynamic": false,
///             "provider_executed": false, "invalid": false },
///   "input": <tool input>,
///   "messages": [<messages of this step>],
///   "tools_context": <tools context or null>
/// }
/// ```
#[must_use]
pub fn default_input(call: &ParsedToolCall, ctx: &ApprovalContext<'_>) -> JsonValue {
    json!({
        "tool": {
            "name": call.tool_name,
            "tool_call_id": call.tool_call_id,
            "dynamic": call.dynamic,
            "provider_executed": call.provider_executed,
            "invalid": call.invalid,
        },
        "input": call.input,
        "messages": serde_json::to_value(ctx.messages).unwrap_or(JsonValue::Null),
        "tools_context": ctx.tools_context.cloned().unwrap_or(JsonValue::Null),
    })
}

/// Approval policy created by [`policy_approval`].
pub struct PolicyApproval<C> {
    client: C,
    path: String,
    to_input: Option<ToInputFn>,
    on_error: FailureMode,
}

/// Resolves tool approvals by evaluating the policy at `path` with `client`.
///
/// The input is [`default_input`] unless [`PolicyApproval::to_input`]
/// replaces it; the result is normalized with [`PolicyDecision::normalize`]
/// and mapped with [`PolicyDecision::into_approval`]. Evaluation errors deny
/// the call with the reason `policy evaluation failed` unless
/// [`PolicyApproval::on_error`] selects [`FailureMode::FallThrough`].
pub fn policy_approval<C: PolicyClient>(client: C, path: impl Into<String>) -> PolicyApproval<C> {
    PolicyApproval {
        client,
        path: path.into(),
        to_input: None,
        on_error: FailureMode::Deny,
    }
}

impl<C> PolicyApproval<C> {
    /// Replaces the default input document.
    #[must_use]
    pub fn to_input(
        mut self,
        f: impl Fn(&ParsedToolCall, &ApprovalContext<'_>) -> JsonValue + Send + Sync + 'static,
    ) -> Self {
        self.to_input = Some(Arc::new(f));
        self
    }

    /// Sets the behaviour on evaluation errors (default: deny).
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

    /// The client.
    #[must_use]
    pub fn client(&self) -> &C {
        &self.client
    }
}

impl<C> fmt::Debug for PolicyApproval<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PolicyApproval")
            .field("path", &self.path)
            .field("custom_input", &self.to_input.is_some())
            .field("on_error", &self.on_error)
            .finish_non_exhaustive()
    }
}

impl<C: PolicyClient> ApprovalPolicy for PolicyApproval<C> {
    fn resolve<'a>(
        &'a self,
        call: &'a ParsedToolCall,
        ctx: ApprovalContext<'a>,
    ) -> BoxFuture<'a, Option<ApprovalStatus>> {
        Box::pin(async move {
            let input = match &self.to_input {
                Some(to_input) => to_input(call, &ctx),
                None => default_input(call, &ctx),
            };
            match self.client.evaluate(&self.path, input).await {
                Ok(raw) => {
                    let decision = PolicyDecision::normalize(&raw);
                    tracing::debug!(
                        tool = %call.tool_name,
                        path = %self.path,
                        decision = crate::diagnostics::decision_kind(&decision),
                        "policy decision"
                    );
                    decision.into_approval()
                }
                Err(_error) => {
                    tracing::warn!(
                        tool = %call.tool_name,
                        path = %self.path,
                        "policy evaluation failed"
                    );
                    match self.on_error {
                        FailureMode::Deny => Some(ApprovalStatus::Denied {
                            reason: Some("policy evaluation failed".to_owned()),
                        }),
                        FailureMode::FallThrough => None,
                    }
                }
            }
        })
    }
}

/// Approval policy created by [`with_default`].
pub struct WithDefault<P> {
    inner: P,
    default: ApprovalStatus,
}

/// Gives calls the inner policy does not decide (`None`) the status
/// `default`, so that every call has a decision.
///
/// Typical use: tools bridged from an MCP server have no `needs_approval`
/// declaration; `with_default(policy, ApprovalStatus::user_approval())` makes
/// unmatched calls wait for a human instead of executing.
pub fn with_default<P: ApprovalPolicy>(policy: P, default: ApprovalStatus) -> WithDefault<P> {
    WithDefault {
        inner: policy,
        default,
    }
}

impl<P> fmt::Debug for WithDefault<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WithDefault")
            .field(
                "default",
                &crate::diagnostics::status_kind(Some(&self.default)),
            )
            .finish_non_exhaustive()
    }
}

impl<P: ApprovalPolicy> ApprovalPolicy for WithDefault<P> {
    fn resolve<'a>(
        &'a self,
        call: &'a ParsedToolCall,
        ctx: ApprovalContext<'a>,
    ) -> BoxFuture<'a, Option<ApprovalStatus>> {
        Box::pin(async move {
            match self.inner.resolve(call, ctx).await {
                Some(status) => Some(status),
                None => Some(self.default.clone()),
            }
        })
    }
}
