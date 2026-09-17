//! Message pruning: drop reasoning, tool calls and empty messages to control
//! context size.
//!
//! Derived from the `pruneMessages` function of the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), translated from TypeScript to Rust and
//! modified; see `NOTICE`. Intended for `prepare_step` callbacks.

use std::collections::HashMap;
use std::collections::HashSet;

use ferrin_spec::ApprovalId;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;

use crate::message::AssistantContent;
use crate::message::Message;
use crate::part::AssistantPart;
use crate::part::ToolPart;

/// Which reasoning parts to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ReasoningPrune {
    /// Keep reasoning.
    #[default]
    None,
    /// Remove reasoning from every assistant message.
    All,
    /// Remove reasoning from every message except the last one.
    BeforeLastMessage,
}

/// Which messages a tool-call rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PruneScope {
    /// Every message.
    All,
    /// Every message except the trailing `n`; tool calls referenced by those
    /// trailing messages are kept everywhere. `n == 0` keeps all tool parts,
    /// matching the reference SDK's `slice(-0)` behavior.
    BeforeLastMessages(usize),
}

impl PruneScope {
    /// Every message except the last one.
    #[must_use]
    pub const fn before_last_message() -> Self {
        Self::BeforeLastMessages(1)
    }
}

/// A rule removing tool calls, results, approval requests and responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallPrune {
    /// Messages the rule applies to.
    pub scope: PruneScope,
    /// Restrict the rule to these tools; `None` prunes every tool.
    pub tools: Option<Vec<ToolName>>,
}

/// What to do with messages left without content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum EmptyMessages {
    /// Keep them.
    Keep,
    /// Remove them (also removes messages that were empty before pruning).
    #[default]
    Remove,
}

/// Options for [`prune`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PruneOptions {
    /// Reasoning removal.
    pub reasoning: ReasoningPrune,
    /// Tool-call rules, applied in order.
    pub tool_calls: Vec<ToolCallPrune>,
    /// Empty message handling.
    pub empty_messages: EmptyMessages,
}

impl PruneOptions {
    /// Default options: keep everything except empty messages.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets reasoning removal.
    #[must_use]
    pub fn reasoning(mut self, reasoning: ReasoningPrune) -> Self {
        self.reasoning = reasoning;
        self
    }

    /// Adds a rule pruning every tool within `scope`.
    #[must_use]
    pub fn tool_calls(mut self, scope: PruneScope) -> Self {
        self.tool_calls.push(ToolCallPrune { scope, tools: None });
        self
    }

    /// Adds a rule pruning only `tools` within `scope`.
    #[must_use]
    pub fn tool_calls_for(
        mut self,
        scope: PruneScope,
        tools: impl IntoIterator<Item = impl Into<ToolName>>,
    ) -> Self {
        self.tool_calls.push(ToolCallPrune {
            scope,
            tools: Some(tools.into_iter().map(Into::into).collect()),
        });
        self
    }

    /// Keeps messages that end up empty.
    #[must_use]
    pub fn keep_empty_messages(mut self) -> Self {
        self.empty_messages = EmptyMessages::Keep;
        self
    }
}

/// Prunes `messages` according to `options`.
#[must_use]
pub fn prune(mut messages: Vec<Message>, options: &PruneOptions) -> Vec<Message> {
    prune_reasoning(&mut messages, options.reasoning);
    for rule in &options.tool_calls {
        prune_tool_calls(&mut messages, rule);
    }
    if options.empty_messages == EmptyMessages::Remove {
        messages.retain(|message| !message.is_empty());
    }
    messages
}

fn prune_reasoning(messages: &mut [Message], reasoning: ReasoningPrune) {
    let last_index = messages.len().saturating_sub(1);
    for (index, message) in messages.iter_mut().enumerate() {
        let keep = match reasoning {
            ReasoningPrune::None => true,
            ReasoningPrune::All => false,
            ReasoningPrune::BeforeLastMessage => index == last_index,
        };
        if keep {
            continue;
        }
        if let Message::Assistant(assistant) = message
            && let AssistantContent::Parts(parts) = &mut assistant.content
        {
            parts.retain(|part| !matches!(part, AssistantPart::Reasoning(_)));
        }
    }
}

enum ToolRef<'a> {
    Call(&'a ToolCallId, &'a ToolName),
    Approval(&'a ApprovalId, Option<&'a ToolCallId>),
}

fn assistant_refs(part: &AssistantPart) -> Option<ToolRef<'_>> {
    match part {
        AssistantPart::ToolCall(call) => Some(ToolRef::Call(&call.tool_call_id, &call.tool_name)),
        AssistantPart::ToolResult(result) => {
            Some(ToolRef::Call(&result.tool_call_id, &result.tool_name))
        }
        AssistantPart::ToolApprovalRequest(request) => Some(ToolRef::Approval(
            &request.approval_id,
            Some(&request.tool_call_id),
        )),
        _ => None,
    }
}

fn tool_refs(part: &ToolPart) -> Option<ToolRef<'_>> {
    match part {
        ToolPart::ToolResult(result) => {
            Some(ToolRef::Call(&result.tool_call_id, &result.tool_name))
        }
        ToolPart::ToolApprovalResponse(response) => {
            Some(ToolRef::Approval(&response.approval_id, None))
        }
    }
}

fn message_refs(message: &Message) -> Vec<ToolRef<'_>> {
    match message {
        Message::Assistant(assistant) => assistant
            .content
            .as_parts()
            .map(|parts| parts.iter().filter_map(assistant_refs).collect())
            .unwrap_or_default(),
        Message::Tool(tool) => tool.content.iter().filter_map(tool_refs).collect(),
        Message::System(_) | Message::User(_) => Vec::new(),
    }
}

struct Kept {
    tool_call_ids: HashSet<ToolCallId>,
    approval_ids: HashSet<ApprovalId>,
    approval_tool_names: HashMap<ApprovalId, ToolName>,
    protected_from: usize,
}

impl Kept {
    fn keeps_call(
        &self,
        tool_call_id: &ToolCallId,
        tool_name: &ToolName,
        rule: &ToolCallPrune,
    ) -> bool {
        self.tool_call_ids.contains(tool_call_id) || rule_keeps(rule, Some(tool_name))
    }

    fn keeps_approval(&self, approval_id: &ApprovalId, rule: &ToolCallPrune) -> bool {
        self.approval_ids.contains(approval_id)
            || rule_keeps(rule, self.approval_tool_names.get(approval_id))
    }
}

/// A part outside the protected tail survives only when the rule targets
/// specific tools and the part's tool is known and not among them.
fn rule_keeps(rule: &ToolCallPrune, tool_name: Option<&ToolName>) -> bool {
    match (&rule.tools, tool_name) {
        (Some(tools), Some(name)) => !tools.contains(name),
        _ => false,
    }
}

fn prune_tool_calls(messages: &mut [Message], rule: &ToolCallPrune) {
    let keep_last = match rule.scope {
        PruneScope::All => None,
        PruneScope::BeforeLastMessages(n) => Some(n),
    };
    let protected_from = keep_last.map_or(messages.len(), |n| {
        if n == 0 {
            0
        } else {
            messages.len().saturating_sub(n)
        }
    });

    let mut kept = Kept {
        tool_call_ids: HashSet::new(),
        approval_ids: HashSet::new(),
        approval_tool_names: HashMap::new(),
        protected_from,
    };
    for message in &messages[protected_from..] {
        for reference in message_refs(message) {
            match reference {
                ToolRef::Call(id, _) => {
                    kept.tool_call_ids.insert(id.clone());
                }
                ToolRef::Approval(id, _) => {
                    kept.approval_ids.insert(id.clone());
                }
            }
        }
    }

    let mut call_tool_names: HashMap<ToolCallId, ToolName> = HashMap::new();
    let mut approval_calls: Vec<(ApprovalId, ToolCallId)> = Vec::new();
    for message in messages.iter() {
        for reference in message_refs(message) {
            match reference {
                ToolRef::Call(id, name) => {
                    call_tool_names.insert(id.clone(), name.clone());
                }
                ToolRef::Approval(approval_id, Some(call_id)) => {
                    approval_calls.push((approval_id.clone(), call_id.clone()));
                }
                ToolRef::Approval(_, None) => {}
            }
        }
    }
    for (approval_id, call_id) in approval_calls {
        if let Some(name) = call_tool_names.get(&call_id) {
            kept.approval_tool_names.insert(approval_id, name.clone());
        }
    }

    for message in messages.iter_mut().take(kept.protected_from) {
        match message {
            Message::Assistant(assistant) => {
                if let AssistantContent::Parts(parts) = &mut assistant.content {
                    parts.retain(|part| match part {
                        AssistantPart::ToolCall(call) => {
                            kept.keeps_call(&call.tool_call_id, &call.tool_name, rule)
                        }
                        AssistantPart::ToolResult(result) => {
                            kept.keeps_call(&result.tool_call_id, &result.tool_name, rule)
                        }
                        AssistantPart::ToolApprovalRequest(request) => {
                            kept.keeps_approval(&request.approval_id, rule)
                        }
                        _ => true,
                    });
                }
            }
            Message::Tool(tool) => {
                tool.content.retain(|part| match part {
                    ToolPart::ToolResult(result) => {
                        kept.keeps_call(&result.tool_call_id, &result.tool_name, rule)
                    }
                    ToolPart::ToolApprovalResponse(response) => {
                        kept.keeps_approval(&response.approval_id, rule)
                    }
                });
            }
            Message::System(_) | Message::User(_) => {}
        }
    }
}
