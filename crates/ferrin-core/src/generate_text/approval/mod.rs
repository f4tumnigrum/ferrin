//! Tool approval: status resolution, request signatures and replay of
//! approval responses from the message history.

use std::collections::HashMap;
use std::fmt;

use ferrin_message::Message;
use ferrin_spec::BoxFuture;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolName;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use serde::Deserialize;
use serde::Serialize;

use super::ParsedToolCall;

pub(crate) mod collect;
pub(crate) mod signature;

/// Outcome of approval resolution for one tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ApprovalStatus {
    /// No approval needed; the tool executes directly.
    NotApplicable,
    /// Approved automatically; the tool executes and the decision is
    /// recorded.
    Approved {
        /// Reason recorded with the decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Denied automatically; the tool does not execute.
    Denied {
        /// Reason recorded with the decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// An external approver must decide; the loop stops with an approval
    /// request.
    UserApproval {
        /// Reason shown to the approver.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl ApprovalStatus {
    /// Approved without reason.
    #[must_use]
    pub fn approved() -> Self {
        Self::Approved { reason: None }
    }

    /// Denied without reason.
    #[must_use]
    pub fn denied() -> Self {
        Self::Denied { reason: None }
    }

    /// User approval without reason.
    #[must_use]
    pub fn user_approval() -> Self {
        Self::UserApproval { reason: None }
    }

    /// Attaches a reason (ignored for `NotApplicable`).
    #[must_use]
    pub fn with_reason(self, reason: impl Into<String>) -> Self {
        let reason = Some(reason.into());
        match self {
            Self::NotApplicable => Self::NotApplicable,
            Self::Approved { .. } => Self::Approved { reason },
            Self::Denied { .. } => Self::Denied { reason },
            Self::UserApproval { .. } => Self::UserApproval { reason },
        }
    }

    /// The reason, if any.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::NotApplicable => None,
            Self::Approved { reason } | Self::Denied { reason } | Self::UserApproval { reason } => {
                reason.as_deref()
            }
        }
    }
}

/// Information available to an [`ApprovalPolicy`].
#[derive(Debug, Clone, Copy)]
pub struct ApprovalContext<'a> {
    /// Messages sent to the model in this step.
    pub messages: &'a [Message],
    /// The tools context of the call.
    pub tools_context: Option<&'a JsonValue>,
    /// Application state for this step, separate from tool execution context.
    pub runtime_context: Option<&'a JsonValue>,
}

/// Decides whether a tool call needs approval.
///
/// The policy runs first; returning `None` falls through to the tool's own
/// `needs_approval` declaration.
pub trait ApprovalPolicy: Send + Sync {
    /// Resolves the approval status of `call`.
    fn resolve<'a>(
        &'a self,
        call: &'a ParsedToolCall,
        ctx: ApprovalContext<'a>,
    ) -> BoxFuture<'a, Option<ApprovalStatus>>;
}

impl ApprovalPolicy for ApprovalStatus {
    fn resolve<'a>(
        &'a self,
        _call: &'a ParsedToolCall,
        _ctx: ApprovalContext<'a>,
    ) -> BoxFuture<'a, Option<ApprovalStatus>> {
        Box::pin(async move { Some(self.clone()) })
    }
}

impl ApprovalPolicy for HashMap<ToolName, ApprovalStatus> {
    fn resolve<'a>(
        &'a self,
        call: &'a ParsedToolCall,
        _ctx: ApprovalContext<'a>,
    ) -> BoxFuture<'a, Option<ApprovalStatus>> {
        let status = self.get(&call.tool_name).cloned();
        Box::pin(async move { status })
    }
}

/// Adapter turning a synchronous closure into an [`ApprovalPolicy`].
pub struct ApprovalPolicyFn<F>(F);

/// Wraps a synchronous closure as an approval policy.
pub fn approval_policy<F>(f: F) -> ApprovalPolicyFn<F>
where
    F: Fn(&ParsedToolCall, &ApprovalContext<'_>) -> Option<ApprovalStatus> + Send + Sync,
{
    ApprovalPolicyFn(f)
}

impl<F> fmt::Debug for ApprovalPolicyFn<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApprovalPolicyFn(..)")
    }
}

impl<F> ApprovalPolicy for ApprovalPolicyFn<F>
where
    F: Fn(&ParsedToolCall, &ApprovalContext<'_>) -> Option<ApprovalStatus> + Send + Sync,
{
    fn resolve<'a>(
        &'a self,
        call: &'a ParsedToolCall,
        ctx: ApprovalContext<'a>,
    ) -> BoxFuture<'a, Option<ApprovalStatus>> {
        let status = (self.0)(call, &ctx);
        Box::pin(async move { status })
    }
}

/// Resolves the approval status: call-level policy first, then the tool's
/// declaration.
pub(crate) async fn resolve_approval(
    call: &ParsedToolCall,
    tool: Option<&Tool>,
    policy: Option<&dyn ApprovalPolicy>,
    ctx: ApprovalContext<'_>,
    tool_ctx: impl FnOnce() -> ToolContext,
) -> ApprovalStatus {
    if let Some(policy) = policy
        && let Some(status) = policy.resolve(call, ctx).await
    {
        return status;
    }
    let Some(tool) = tool else {
        return ApprovalStatus::NotApplicable;
    };
    if !tool.needs_approval().is_declared() {
        return ApprovalStatus::NotApplicable;
    }
    if tool
        .needs_approval()
        .resolve(call.input.clone(), tool_ctx())
        .await
    {
        ApprovalStatus::UserApproval { reason: None }
    } else {
        ApprovalStatus::NotApplicable
    }
}
