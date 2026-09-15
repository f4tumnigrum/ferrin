//! Shadow mode: observe policy decisions before enforcing them.

use std::fmt;
use std::sync::Arc;

use ferrin_core::generate_text::ApprovalContext;
use ferrin_core::generate_text::ApprovalPolicy;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_core::generate_text::ParsedToolCall;
use ferrin_spec::BoxFuture;

/// Whether a [`Shadow`] policy acts on the decisions it observes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Enforcement {
    /// Report decisions and return `None`, leaving the tool's own
    /// `needs_approval` in charge.
    #[default]
    Observe,
    /// Report and return the decisions.
    Enforce,
}

/// Receives every decision of the wrapped policy.
pub type OnDecisionFn = Arc<dyn Fn(&ParsedToolCall, Option<&ApprovalStatus>) + Send + Sync>;

/// Approval policy created by [`shadow`].
pub struct Shadow<P> {
    inner: P,
    enforcement: Enforcement,
    on_decision: Option<OnDecisionFn>,
}

/// Evaluates `policy` for every call and reports its decision through
/// [`Shadow::on_decision`], but only acts on it under
/// [`Enforcement::Enforce`]. Roll a policy out by observing first and
/// flipping the enforcement later without changing the wiring.
pub fn shadow<P: ApprovalPolicy>(policy: P) -> Shadow<P> {
    Shadow {
        inner: policy,
        enforcement: Enforcement::Observe,
        on_decision: None,
    }
}

impl<P> Shadow<P> {
    /// Sets whether decisions are enforced (default: observe only).
    #[must_use]
    pub fn enforcement(mut self, enforcement: Enforcement) -> Self {
        self.enforcement = enforcement;
        self
    }

    /// Registers the decision callback.
    #[must_use]
    pub fn on_decision(
        mut self,
        f: impl Fn(&ParsedToolCall, Option<&ApprovalStatus>) + Send + Sync + 'static,
    ) -> Self {
        self.on_decision = Some(Arc::new(f));
        self
    }
}

impl<P> fmt::Debug for Shadow<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shadow")
            .field("enforcement", &self.enforcement)
            .field("on_decision", &self.on_decision.is_some())
            .finish_non_exhaustive()
    }
}

impl<P: ApprovalPolicy> ApprovalPolicy for Shadow<P> {
    fn resolve<'a>(
        &'a self,
        call: &'a ParsedToolCall,
        ctx: ApprovalContext<'a>,
    ) -> BoxFuture<'a, Option<ApprovalStatus>> {
        Box::pin(async move {
            let status = self.inner.resolve(call, ctx).await;
            tracing::debug!(
                tool = %call.tool_name,
                status = crate::diagnostics::status_kind(status.as_ref()),
                enforcement = ?self.enforcement,
                "shadow policy decision"
            );
            if let Some(on_decision) = &self.on_decision {
                on_decision(call, status.as_ref());
            }
            match self.enforcement {
                Enforcement::Observe => None,
                Enforcement::Enforce => status,
            }
        })
    }
}
