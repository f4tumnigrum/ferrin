//! The result of `generate_text`.

use ferrin_message::Message;
use ferrin_spec::FinishReason;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::Usage;
use ferrin_spec::Warning;

use super::StepResult;

/// Result of a completed generation loop.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerateTextResult<O> {
    /// All steps in order (at least one).
    pub steps: Vec<StepResult>,
    /// Usage summed over all steps.
    pub total_usage: Usage,
    /// The structured output (`()` when none was requested).
    pub output: O,
}

impl<O> GenerateTextResult<O> {
    /// The final step.
    ///
    /// # Panics
    ///
    /// Never: the loop always records at least one step.
    #[must_use]
    pub fn last_step(&self) -> &StepResult {
        #[allow(clippy::expect_used, reason = "the loop records at least one step")]
        self.steps.last().expect("at least one step")
    }

    /// Text of the final step.
    #[must_use]
    pub fn text(&self) -> String {
        self.last_step().text()
    }

    /// Reasoning text of the final step.
    #[must_use]
    pub fn reasoning_text(&self) -> Option<String> {
        self.last_step().reasoning_text()
    }

    /// Finish reason of the final step.
    #[must_use]
    pub fn finish_reason(&self) -> &FinishReason {
        &self.last_step().finish_reason
    }

    /// Usage of the final step.
    #[must_use]
    pub fn usage(&self) -> &Usage {
        &self.last_step().usage
    }

    /// Provider metadata of the final step.
    #[must_use]
    pub fn provider_metadata(&self) -> Option<&ProviderMetadata> {
        self.last_step().provider_metadata.as_ref()
    }

    /// Warnings of the final step.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.last_step().warnings
    }

    /// Messages of all steps, ready to append to the conversation history.
    #[must_use]
    pub fn response_messages(&self) -> Vec<Message> {
        self.steps
            .iter()
            .flat_map(|step| step.response.messages.iter().cloned())
            .collect()
    }

    /// Maps the output.
    pub fn map_output<P>(self, f: impl FnOnce(O) -> P) -> GenerateTextResult<P> {
        GenerateTextResult {
            steps: self.steps,
            total_usage: self.total_usage,
            output: f(self.output),
        }
    }
}
