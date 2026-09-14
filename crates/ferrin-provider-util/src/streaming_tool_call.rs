//! Tracks partial tool calls while streaming so `tool-input-start` and
//! `tool-input-delta` parts are emitted before the final `tool-call`.

use std::collections::HashSet;

use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;

/// Remembers which tool calls already emitted `tool-input-start`.
#[derive(Debug, Default, Clone)]
pub struct StreamingToolCallTracker {
    started: HashSet<ToolCallId>,
}

impl StreamingToolCallTracker {
    /// Creates an empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if `id` already started.
    #[must_use]
    pub fn has_started(&self, id: &ToolCallId) -> bool {
        self.started.contains(id)
    }

    /// Marks `id` as started, returning `true` the first time.
    pub fn start(&mut self, id: ToolCallId) -> bool {
        self.started.insert(id)
    }

    /// Emits the parts a tool call needs so consumers see a consistent
    /// `tool-input-start` / (`tool-input-delta`) / `tool-input-end` /
    /// `tool-call` sequence, even when the provider delivered the call whole.
    pub fn parts_for_complete_call(
        &mut self,
        id: ToolCallId,
        name: ToolName,
        input: String,
        provider_executed: bool,
    ) -> Vec<StreamPart> {
        let mut parts = Vec::with_capacity(4);
        if self.start(id.clone()) {
            parts.push(StreamPart::ToolInputStart {
                id: id.clone(),
                tool_name: name.clone(),
                provider_executed,
                dynamic: false,
                title: None,
                provider_metadata: None,
            });
            if !input.is_empty() {
                parts.push(StreamPart::ToolInputDelta {
                    id: id.clone(),
                    delta: input.clone(),
                    provider_metadata: None,
                });
            }
        }
        parts.push(StreamPart::ToolInputEnd {
            id: id.clone(),
            provider_metadata: None,
        });
        let mut call = ToolCall::new(id, name, input);
        call.provider_executed = provider_executed;
        parts.push(StreamPart::ToolCall(call));
        parts
    }
}
