//! Shadow mode: observe policy decisions before enforcing them.
//!
//! Derived from the Vercel AI SDK shadow policy (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), reimplemented with owned Rust tasks.

use std::fmt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Arc;
use std::sync::Mutex;

use chrono::DateTime;
use chrono::Utc;
use ferrin_core::generate_text::ApprovalContext;
use ferrin_core::generate_text::ApprovalPolicy;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_core::generate_text::ParsedToolCall;
use ferrin_spec::BoxFuture;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use serde::Deserialize;
use serde::Serialize;
use tokio::task::JoinSet;

/// Whether a [`Shadow`] policy acts on the decisions it observes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Enforcement {
    /// Report decisions and approve execution, overriding tool-defined approval.
    #[default]
    Observe,
    /// Report and return the decisions.
    Enforce,
}

/// Identifying information for the tool call a policy evaluated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyDecisionToolCall {
    /// Registered name of the tool.
    pub tool_name: ToolName,
    /// Identifier of this tool invocation.
    pub tool_call_id: ToolCallId,
    /// Parsed tool input submitted for approval.
    pub input: JsonValue,
}

/// The observed and effective approval decisions for one tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyDecisionEvent {
    /// The evaluated tool invocation.
    pub tool_call: PolicyDecisionToolCall,
    /// Normalized decision returned by the wrapped policy.
    pub decision: ApprovalStatus,
    /// Whether the effective decision enforces the wrapped policy.
    pub enforced: bool,
    /// Status the generation loop will act on.
    pub effective: ApprovalStatus,
    /// UTC evaluation time, serialized as an ISO 8601 string.
    pub timestamp: DateTime<Utc>,
}

/// Asynchronous observer receiving each evaluated and effective decision.
pub type OnDecisionFn = Arc<dyn Fn(PolicyDecisionEvent) -> BoxFuture<'static, ()> + Send + Sync>;

/// Synchronous low-level observer of the original policy return value.
pub type OnDecisionSyncFn = Arc<dyn Fn(&ParsedToolCall, Option<&ApprovalStatus>) + Send + Sync>;

enum Observer {
    Async(OnDecisionFn),
    Sync(OnDecisionSyncFn),
}

/// Approval policy created by [`shadow`].
pub struct Shadow<P> {
    inner: P,
    enforcement: Enforcement,
    on_decision: Option<Observer>,
    audit_tasks: Mutex<JoinSet<()>>,
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
        audit_tasks: Mutex::new(JoinSet::new()),
    }
}

impl<P> Shadow<P> {
    /// Sets whether decisions are enforced (default: observe only).
    #[must_use]
    pub fn enforcement(mut self, enforcement: Enforcement) -> Self {
        self.enforcement = enforcement;
        self
    }

    /// Registers an asynchronous decision observer, replacing any prior observer.
    ///
    /// The observer runs independently on the current Tokio runtime; its output
    /// and task panics are ignored so auditing cannot change approval. Without a
    /// Tokio runtime the event is skipped. Dropping the policy cancels unfinished
    /// observers; call [`Self::flush_decisions`] to drain queued events first.
    ///
    /// # Examples
    ///
    /// ```
    /// use ferrin_core::generate_text::ApprovalStatus;
    /// use ferrin_policy::shadow;
    ///
    /// let policy = shadow(ApprovalStatus::denied()).on_decision(|event| async move {
    ///     assert!(!event.enforced);
    ///     assert_eq!(event.effective, ApprovalStatus::approved());
    /// });
    /// ```
    #[must_use]
    pub fn on_decision<F, Fut>(mut self, observer: F) -> Self
    where
        F: Fn(PolicyDecisionEvent) -> Fut + Send + Sync + 'static,
        Fut: Future + Send + 'static,
    {
        let observer = Arc::new(observer);
        self.on_decision = Some(Observer::Async(Arc::new(move |event| {
            let observer = Arc::clone(&observer);
            Box::pin(async move {
                let _ = observer(event).await;
            })
        })));
        self
    }

    /// Registers a synchronous low-level observer, replacing any prior observer.
    ///
    /// This Ferrin extension receives the raw optional status and blocks approval
    /// until it returns. Unwinding observer panics are ignored. Prefer
    /// [`Self::on_decision`] for normalized events and independent execution.
    #[must_use]
    pub fn on_decision_sync(
        mut self,
        observer: impl Fn(&ParsedToolCall, Option<&ApprovalStatus>) + Send + Sync + 'static,
    ) -> Self {
        self.on_decision = Some(Observer::Sync(Arc::new(observer)));
        self
    }

    /// Waits for audit callbacks queued before this call took their task set.
    ///
    /// Concurrent approval resolution can queue new events without waiting for
    /// this flush. Cancelling the flush cancels the callbacks it took ownership of.
    pub async fn flush_decisions(&self) {
        let mut pending = {
            let mut tasks = self
                .audit_tasks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *tasks)
        };
        while pending.join_next().await.is_some() {}
    }

    fn report(
        &self,
        call: &ParsedToolCall,
        status: Option<&ApprovalStatus>,
        effective: &ApprovalStatus,
    ) {
        match &self.on_decision {
            Some(Observer::Async(observer)) => {
                let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                    return;
                };
                let event = PolicyDecisionEvent {
                    tool_call: PolicyDecisionToolCall {
                        tool_name: call.tool_name.clone(),
                        tool_call_id: call.tool_call_id.clone(),
                        input: call.input.clone(),
                    },
                    decision: status.cloned().unwrap_or(ApprovalStatus::NotApplicable),
                    enforced: self.enforcement == Enforcement::Enforce,
                    effective: effective.clone(),
                    timestamp: Utc::now(),
                };
                let observer = Arc::clone(observer);
                let mut tasks = self
                    .audit_tasks
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                while tasks.try_join_next().is_some() {}
                tasks.spawn_on(async move { observer(event).await }, &runtime);
            }
            Some(Observer::Sync(observer)) => {
                let _ = catch_unwind(AssertUnwindSafe(|| observer(call, status)));
            }
            None => {}
        }
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
            let effective = match self.enforcement {
                Enforcement::Observe => ApprovalStatus::approved(),
                Enforcement::Enforce => status.clone().unwrap_or(ApprovalStatus::NotApplicable),
            };
            self.report(call, status.as_ref(), &effective);
            Some(effective)
        })
    }
}
