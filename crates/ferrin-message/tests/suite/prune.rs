use ferrin_message::AssistantPart;
use ferrin_message::Message;
use ferrin_message::ToolApprovalRequest;
use ferrin_message::ToolApprovalResponse;
use ferrin_message::ToolCallPart;
use ferrin_message::ToolPart;
use ferrin_message::ToolResultOutput;
use ferrin_message::ToolResultPart;
use ferrin_message::prune::PruneOptions;
use ferrin_message::prune::PruneScope;
use ferrin_message::prune::ReasoningPrune;
use ferrin_message::prune::prune;
use pretty_assertions::assert_eq;
use serde_json::json;

fn call(id: &str, tool: &str) -> AssistantPart {
    AssistantPart::ToolCall(ToolCallPart {
        tool_call_id: id.into(),
        tool_name: tool.into(),
        input: json!({}),
        provider_executed: false,
        provider_options: None,
    })
}

fn result(id: &str, tool: &str) -> ToolPart {
    ToolPart::ToolResult(ToolResultPart {
        tool_call_id: id.into(),
        tool_name: tool.into(),
        output: ToolResultOutput::text("ok"),
        provider_options: None,
    })
}

fn conversation() -> Vec<Message> {
    vec![
        Message::user("Weather in Tokyo and Busan?"),
        Message::assistant_parts([
            AssistantPart::reasoning("first"),
            call("call-1", "weather-1"),
            call("call-2", "weather-2"),
            AssistantPart::ToolApprovalRequest(ToolApprovalRequest::new("approval-1", "call-2")),
        ]),
        Message::tool([
            result("call-1", "weather-1"),
            ToolPart::ToolApprovalResponse(ToolApprovalResponse::approved("approval-1")),
        ]),
        Message::tool([result("call-2", "weather-2")]),
        Message::assistant_parts([AssistantPart::reasoning("last")]),
    ]
}

#[test]
fn prunes_reasoning() {
    let all = prune(
        conversation(),
        &PruneOptions::new().reasoning(ReasoningPrune::All),
    );
    assert_eq!(
        all.len(),
        4,
        "the trailing reasoning-only message is removed"
    );
    assert!(
        all.iter()
            .filter_map(Message::as_assistant)
            .flat_map(|m| m.content.as_parts().unwrap())
            .all(|part| !matches!(part, AssistantPart::Reasoning(_)))
    );

    let before_last = prune(
        conversation(),
        &PruneOptions::new().reasoning(ReasoningPrune::BeforeLastMessage),
    );
    assert_eq!(before_last.len(), 5);
    assert_eq!(
        before_last[4]
            .as_assistant()
            .unwrap()
            .content
            .as_parts()
            .unwrap(),
        &[AssistantPart::reasoning("last")]
    );
    assert_eq!(
        before_last[1]
            .as_assistant()
            .unwrap()
            .content
            .as_parts()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn prunes_all_tool_calls_and_approvals() {
    let pruned = prune(
        conversation(),
        &PruneOptions::new().tool_calls(PruneScope::All),
    );
    assert_eq!(
        pruned,
        vec![
            Message::user("Weather in Tokyo and Busan?"),
            Message::assistant_parts([AssistantPart::reasoning("first")]),
            Message::assistant_parts([AssistantPart::reasoning("last")]),
        ]
    );
}

#[test]
fn keeps_tool_calls_referenced_by_trailing_messages() {
    let messages = vec![
        Message::user("q"),
        Message::assistant_parts([call("call-1", "a")]),
        Message::tool([result("call-1", "a")]),
        Message::assistant_parts([call("call-2", "b")]),
        Message::tool([result("call-2", "b")]),
    ];
    let pruned = prune(
        messages.clone(),
        &PruneOptions::new().tool_calls(PruneScope::before_last_message()),
    );
    assert_eq!(
        pruned,
        vec![
            Message::user("q"),
            Message::assistant_parts([call("call-2", "b")]),
            Message::tool([result("call-2", "b")]),
        ]
    );

    let two = prune(
        messages.clone(),
        &PruneOptions::new().tool_calls(PruneScope::BeforeLastMessages(2)),
    );
    assert_eq!(two, pruned);

    let zero = prune(
        messages,
        &PruneOptions::new().tool_calls(PruneScope::BeforeLastMessages(0)),
    );
    assert_eq!(zero.len(), 1, "zero trailing messages behaves like `All`");
}

#[test]
fn selective_pruning_removes_approvals_with_their_tool() {
    let pruned = prune(
        conversation(),
        &PruneOptions::new().tool_calls_for(PruneScope::All, ["weather-2"]),
    );
    assert_eq!(
        pruned,
        vec![
            Message::user("Weather in Tokyo and Busan?"),
            Message::assistant_parts([
                AssistantPart::reasoning("first"),
                call("call-1", "weather-1")
            ]),
            Message::tool([result("call-1", "weather-1")]),
            Message::assistant_parts([AssistantPart::reasoning("last")]),
        ]
    );
}

#[test]
fn selective_pruning_drops_unresolved_approval_responses() {
    let messages = vec![
        Message::user("q"),
        Message::assistant_parts([call("call-1", "weather-1")]),
        Message::tool([
            ToolPart::ToolApprovalResponse(ToolApprovalResponse::approved("unknown")),
            result("call-1", "weather-1"),
        ]),
    ];
    let pruned = prune(
        messages,
        &PruneOptions::new().tool_calls_for(PruneScope::All, ["weather-2"]),
    );
    assert_eq!(
        pruned,
        vec![
            Message::user("q"),
            Message::assistant_parts([call("call-1", "weather-1")]),
            Message::tool([result("call-1", "weather-1")]),
        ]
    );
}

#[test]
fn empty_messages_can_be_kept() {
    let messages = vec![
        Message::user(""),
        Message::assistant_parts(Vec::<AssistantPart>::new()),
    ];
    assert!(prune(messages.clone(), &PruneOptions::new()).is_empty());
    assert_eq!(
        prune(messages.clone(), &PruneOptions::new().keep_empty_messages()),
        messages
    );
}

#[test]
fn protected_tail_keeps_the_complete_approval_chain() {
    let chain = [
        Message::assistant_parts([call("call-1", "weather")]),
        Message::assistant_parts([AssistantPart::ToolApprovalRequest(
            ToolApprovalRequest::new("approval-1", "call-1"),
        )]),
        Message::tool([ToolPart::ToolApprovalResponse(
            ToolApprovalResponse::approved("approval-1"),
        )]),
        Message::tool([result("call-1", "weather")]),
    ];
    // Each boundary protects a request, response, or result respectively.
    for end in 2..=chain.len() {
        let messages = chain[..end].to_vec();
        for options in [
            PruneOptions::new().tool_calls(PruneScope::before_last_message()),
            PruneOptions::new().tool_calls_for(PruneScope::before_last_message(), ["weather"]),
        ] {
            assert_eq!(prune(messages.clone(), &options), messages);
        }
    }
}
