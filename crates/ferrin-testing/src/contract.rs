//! Checks a provider stream against the specification contract.
//!
//! Rules: the first part is `stream-start`; exactly one terminal part
//! (`finish` or `error`) ends the stream; text, reasoning and tool-input
//! parts are properly paired (start before delta/end, no duplicate starts).

use std::collections::HashSet;

use ferrin_spec::PartId;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCallId;
use ferrin_spec::language_model::StreamResult;
use futures_util::StreamExt;

/// One contract violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractViolation {
    /// Index of the offending part (or of the end of the stream).
    pub index: usize,
    /// Description.
    pub message: String,
}

impl std::fmt::Display for ContractViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "part {}: {}", self.index, self.message)
    }
}

/// Incremental contract checker.
#[derive(Debug, Default)]
pub struct StreamContractChecker {
    index: usize,
    started: bool,
    terminated: bool,
    seen_parts: HashSet<PartId>,
    seen_tool_inputs: HashSet<ToolCallId>,
    open_text: HashSet<PartId>,
    open_reasoning: HashSet<PartId>,
    open_tool_inputs: HashSet<ToolCallId>,
    violations: Vec<ContractViolation>,
}

impl StreamContractChecker {
    /// Creates a checker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn violation(&mut self, message: impl Into<String>) {
        self.violations.push(ContractViolation {
            index: self.index,
            message: message.into(),
        });
    }

    /// Observes one part.
    pub fn observe(&mut self, part: &StreamPart) {
        if self.index == 0 && !matches!(part, StreamPart::StreamStart { .. }) {
            self.violation(format!(
                "stream must start with stream-start, got {}",
                part.kind_name()
            ));
        }
        if self.terminated {
            self.violation(format!("{} after the terminal part", part.kind_name()));
        }
        match part {
            StreamPart::StreamStart { .. } => {
                if self.started {
                    self.violation("duplicate stream-start");
                }
                self.started = true;
            }
            StreamPart::TextStart { id, .. } => {
                self.open_text.insert(id.clone());
                if !self.seen_parts.insert(id.clone()) {
                    self.violation(format!("text part `{id}` started twice"));
                }
            }
            StreamPart::TextDelta { id, .. } => {
                if !self.open_text.contains(id) {
                    self.violation(format!("text-delta for closed part `{id}`"));
                }
            }
            StreamPart::TextEnd { id, .. } => {
                if !self.open_text.remove(id) {
                    self.violation(format!("text-end for closed part `{id}`"));
                }
            }
            StreamPart::ReasoningStart { id, .. } => {
                self.open_reasoning.insert(id.clone());
                if !self.seen_parts.insert(id.clone()) {
                    self.violation(format!("reasoning part `{id}` started twice"));
                }
            }
            StreamPart::ReasoningDelta { id, .. } => {
                if !self.open_reasoning.contains(id) {
                    self.violation(format!("reasoning-delta for closed part `{id}`"));
                }
            }
            StreamPart::ReasoningEnd { id, .. } => {
                if !self.open_reasoning.remove(id) {
                    self.violation(format!("reasoning-end for closed part `{id}`"));
                }
            }
            StreamPart::ToolInputStart { id, .. } => {
                self.open_tool_inputs.insert(id.clone());
                if !self.seen_tool_inputs.insert(id.clone()) {
                    self.violation(format!("tool input `{id}` started twice"));
                }
            }
            StreamPart::ToolInputDelta { id, .. } => {
                if !self.open_tool_inputs.contains(id) {
                    self.violation(format!("tool-input-delta for unknown tool input `{id}`"));
                }
            }
            StreamPart::ToolInputEnd { id, .. } => {
                if !self.open_tool_inputs.remove(id) {
                    self.violation(format!("tool-input-end for unknown tool input `{id}`"));
                }
            }
            StreamPart::Finish { .. } | StreamPart::Error { .. } => {
                self.terminated = true;
            }
            _ => {}
        }
        self.index += 1;
    }

    /// Finishes the check.
    ///
    /// # Errors
    ///
    /// Returns every violation, including unterminated streams and open parts.
    pub fn finish(mut self) -> Result<(), Vec<ContractViolation>> {
        if self.index == 0 {
            self.violation("stream is empty");
        } else if !self.terminated {
            self.violation("stream ended without finish or error");
        }
        for id in std::mem::take(&mut self.open_text) {
            self.violation(format!("text part `{id}` never ended"));
        }
        for id in std::mem::take(&mut self.open_reasoning) {
            self.violation(format!("reasoning part `{id}` never ended"));
        }
        for id in std::mem::take(&mut self.open_tool_inputs) {
            self.violation(format!("tool input `{id}` never ended"));
        }
        if self.violations.is_empty() {
            Ok(())
        } else {
            Err(self.violations)
        }
    }

    /// Checks a slice of parts.
    ///
    /// # Errors
    ///
    /// See [`finish`](Self::finish).
    pub fn check(parts: &[StreamPart]) -> Result<(), Vec<ContractViolation>> {
        let mut checker = Self::new();
        for part in parts {
            checker.observe(part);
        }
        checker.finish()
    }

    /// Drains a stream result, returning the parts and the check outcome.
    pub async fn check_stream(
        result: StreamResult,
    ) -> (Vec<StreamPart>, Result<(), Vec<ContractViolation>>) {
        let mut checker = Self::new();
        let mut parts = Vec::new();
        let mut stream = result.stream;
        while let Some(part) = stream.next().await {
            checker.observe(&part);
            parts.push(part);
        }
        (parts, checker.finish())
    }
}
