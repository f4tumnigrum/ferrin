//! The result of `generate_text`.

use ferrin_message::Message;
use ferrin_spec::FinishReason;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::Source;

use super::GeneratedFile;
use super::ParsedToolCall;
use super::StepContent;
use super::StepRequest;
use super::StepResponse;
use super::ToolResult;

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
    /// Panics if callers manually construct or mutate a result with no steps.
    /// Generation loops always record at least one step before returning a result.
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

    /// Token usage summed over every step.
    ///
    /// Use `last_step().usage` for the final model call alone.
    #[must_use]
    pub fn usage(&self) -> &Usage {
        &self.total_usage
    }

    /// Provider metadata of the final step.
    #[must_use]
    pub fn provider_metadata(&self) -> Option<&ProviderMetadata> {
        self.last_step().provider_metadata.as_ref()
    }

    /// Warnings from every step in generation order.
    #[must_use]
    pub fn warnings(&self) -> Vec<&Warning> {
        self.steps.iter().flat_map(|step| &step.warnings).collect()
    }

    /// Iterates over content from every step in generation order.
    pub fn content(&self) -> impl Iterator<Item = &StepContent> {
        self.steps.iter().flat_map(|step| &step.content)
    }

    /// Iterates over files generated in every step.
    pub fn files(&self) -> impl Iterator<Item = &GeneratedFile> {
        self.steps.iter().flat_map(StepResult::files)
    }

    /// Iterates over citation sources from every step.
    pub fn sources(&self) -> impl Iterator<Item = &Source> {
        self.steps.iter().flat_map(StepResult::sources)
    }

    /// Iterates over parsed tool calls from every step.
    pub fn tool_calls(&self) -> impl Iterator<Item = &ParsedToolCall> {
        self.steps.iter().flat_map(StepResult::tool_calls)
    }

    /// Iterates over static tool calls from every step.
    pub fn static_tool_calls(&self) -> impl Iterator<Item = &ParsedToolCall> {
        self.tool_calls().filter(|call| !call.dynamic)
    }

    /// Iterates over dynamic tool calls from every step.
    pub fn dynamic_tool_calls(&self) -> impl Iterator<Item = &ParsedToolCall> {
        self.tool_calls().filter(|call| call.dynamic)
    }

    /// Iterates over final tool results from every step.
    pub fn tool_results(&self) -> impl Iterator<Item = &ToolResult> {
        self.steps.iter().flat_map(StepResult::tool_results)
    }

    /// Iterates over static tool results from every step.
    pub fn static_tool_results(&self) -> impl Iterator<Item = &ToolResult> {
        self.tool_results().filter(|result| !result.dynamic)
    }

    /// Iterates over dynamic tool results from every step.
    pub fn dynamic_tool_results(&self) -> impl Iterator<Item = &ToolResult> {
        self.tool_results().filter(|result| result.dynamic)
    }

    /// The final step, using the reference SDK's naming.
    #[must_use]
    pub fn final_step(&self) -> &StepResult {
        self.last_step()
    }

    /// The provider's unnormalized finish reason from the final step.
    #[must_use]
    pub fn raw_finish_reason(&self) -> Option<&str> {
        self.finish_reason().raw.as_deref()
    }

    /// Request metadata from the final step.
    #[must_use]
    pub fn request(&self) -> &StepRequest {
        &self.last_step().request
    }

    /// Response metadata and messages from the final step.
    #[must_use]
    pub fn response(&self) -> &StepResponse {
        &self.last_step().response
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
