//! Stop conditions of the generation loop.

use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::ToolName;

use super::StepResult;

/// Decides after each step whether the loop stops.
///
/// Conditions configured on a call are combined with *any-of* semantics.
/// Implemented for every `Fn(&[StepResult]) -> bool` closure.
pub trait StopCondition: Send + Sync {
    /// Returns `true` when the loop must stop after `steps`.
    fn should_stop<'a>(&'a self, steps: &'a [StepResult]) -> BoxFuture<'a, bool>;
}

impl<F> StopCondition for F
where
    F: Fn(&[StepResult]) -> bool + Send + Sync,
{
    fn should_stop<'a>(&'a self, steps: &'a [StepResult]) -> BoxFuture<'a, bool> {
        let result = self(steps);
        Box::pin(async move { result })
    }
}

/// Stops when the number of completed steps equals `n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepCount(u32);

/// Stops when `n` steps have been completed.
#[must_use]
pub fn step_count(n: u32) -> StepCount {
    StepCount(n)
}

impl StopCondition for StepCount {
    fn should_stop<'a>(&'a self, steps: &'a [StepResult]) -> BoxFuture<'a, bool> {
        let stop = u64::try_from(steps.len()).unwrap_or(u64::MAX) == u64::from(self.0);
        Box::pin(async move { stop })
    }
}

/// Stops when the last step called one of the listed tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HasToolCall(Vec<ToolName>);

/// Stops when the last step contains a call to `tool_name`.
#[must_use]
pub fn has_tool_call(tool_name: impl Into<ToolName>) -> HasToolCall {
    HasToolCall(vec![tool_name.into()])
}

/// Stops when the last step contains a call to any of `tool_names`.
#[must_use]
pub fn has_any_tool_call(tool_names: impl IntoIterator<Item = impl Into<ToolName>>) -> HasToolCall {
    HasToolCall(tool_names.into_iter().map(Into::into).collect())
}

impl StopCondition for HasToolCall {
    fn should_stop<'a>(&'a self, steps: &'a [StepResult]) -> BoxFuture<'a, bool> {
        let stop = steps.last().is_some_and(|step| {
            step.tool_calls()
                .any(|call| self.0.contains(&call.tool_name))
        });
        Box::pin(async move { stop })
    }
}

/// Never stops on its own (the loop ends when no tool calls remain).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Never;

/// A condition that never stops the loop.
#[must_use]
pub fn never() -> Never {
    Never
}

impl StopCondition for Never {
    fn should_stop<'a>(&'a self, _steps: &'a [StepResult]) -> BoxFuture<'a, bool> {
        Box::pin(async { false })
    }
}

/// Evaluates `conditions` with any-of semantics.
pub(crate) async fn is_stop_condition_met(
    conditions: &[Arc<dyn StopCondition>],
    steps: &[StepResult],
) -> bool {
    futures_util::future::join_all(
        conditions
            .iter()
            .map(|condition| condition.should_stop(steps)),
    )
    .await
    .into_iter()
    .any(std::convert::identity)
}
