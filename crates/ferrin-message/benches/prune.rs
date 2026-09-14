//! Message pruning over a long conversation with reasoning, tool calls and
//! tool results.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benchmark code may panic on unexpected values"
)]

use std::hint::black_box;

use criterion::BatchSize;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use ferrin_message::AssistantPart;
use ferrin_message::Message;
use ferrin_message::ToolCallPart;
use ferrin_message::ToolPart;
use ferrin_message::ToolResultOutput;
use ferrin_message::ToolResultPart;
use ferrin_message::prune::PruneOptions;
use ferrin_message::prune::PruneScope;
use ferrin_message::prune::ReasoningPrune;
use ferrin_message::prune::prune;
use serde_json::json;

/// Conversation rounds; each round is four messages.
const ROUNDS: [usize; 2] = [10, 50];

fn call(id: &str, tool: &str) -> AssistantPart {
    AssistantPart::ToolCall(ToolCallPart {
        tool_call_id: id.into(),
        tool_name: tool.into(),
        input: json!({ "city": "Tokyo", "unit": "celsius" }),
        provider_executed: false,
        provider_options: None,
    })
}

fn result(id: &str, tool: &str) -> ToolPart {
    ToolPart::ToolResult(ToolResultPart {
        tool_call_id: id.into(),
        tool_name: tool.into(),
        output: ToolResultOutput::text("{\"temperature\": 21, \"sky\": \"clear\"}"),
        provider_options: None,
    })
}

/// `rounds` rounds of user question, assistant reasoning plus two tool
/// calls, tool results, assistant answer.
fn conversation(rounds: usize) -> Vec<Message> {
    let mut messages = Vec::with_capacity(rounds * 4);
    for round in 0..rounds {
        let first = format!("call-{round}-1");
        let second = format!("call-{round}-2");
        messages.push(Message::user(format!(
            "Weather in Tokyo and Busan, round {round}?"
        )));
        messages.push(Message::assistant_parts([
            AssistantPart::reasoning("The user wants two cities; call the tool twice."),
            call(&first, "weather"),
            call(&second, "weather"),
        ]));
        messages.push(Message::tool([
            result(&first, "weather"),
            result(&second, "weather"),
        ]));
        messages.push(Message::assistant(
            "Tokyo is 21 °C and clear; Busan is 19 °C and cloudy.",
        ));
    }
    messages
}

fn bench_prune(c: &mut Criterion) {
    let variants: [(&str, PruneOptions); 4] = [
        ("none", PruneOptions::new()),
        (
            "reasoning_all",
            PruneOptions::new().reasoning(ReasoningPrune::All),
        ),
        (
            "tool_calls_all",
            PruneOptions::new().tool_calls(PruneScope::All),
        ),
        (
            "tool_calls_before_last_4",
            PruneOptions::new()
                .reasoning(ReasoningPrune::All)
                .tool_calls(PruneScope::BeforeLastMessages(4)),
        ),
    ];
    let mut group = c.benchmark_group("prune");
    for rounds in ROUNDS {
        let history = conversation(rounds);
        group.throughput(Throughput::Elements(u64::try_from(history.len()).unwrap()));
        for (label, options) in &variants {
            group.bench_with_input(
                BenchmarkId::new(*label, history.len()),
                &history,
                |b, history| {
                    b.iter_batched(
                        || history.clone(),
                        |history| black_box(prune(history, options).len()),
                        BatchSize::LargeInput,
                    );
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, bench_prune);
criterion_main!(benches);
