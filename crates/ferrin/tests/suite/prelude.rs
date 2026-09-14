//! The prelude covers a generate, a stream and a tool-loop call.

use std::sync::Arc;

use ferrin::prelude::*;
use ferrin_testing::MockLanguageModel;
use ferrin_testing::text_parts;
use pretty_assertions::assert_eq;

fn text_result(text: &str) -> GenerateResult {
    GenerateResult::new(vec![Content::text(text)], FinishReason::stop())
}

#[tokio::test]
async fn generate_text_runs_against_a_mock_model() {
    let model = MockLanguageModel::builder()
        .provider("mock")
        .model_id("mock-model")
        .generate(text_result("hello"))
        .build_shared();
    let result = generate_text(Arc::clone(&model))
        .system("Be brief.")
        .prompt("Say hello")
        .await
        .unwrap();
    assert_eq!(result.text(), "hello");
    assert_eq!(result.steps.len(), 1);
    assert_eq!(model.call_count(), 1);
}

#[tokio::test]
async fn stream_text_yields_text_deltas() {
    let model = MockLanguageModel::builder()
        .stream(text_parts(["a", "b", "c"], Usage::default()))
        .build_shared();
    let result = stream_text(model).prompt("hi").await.unwrap();
    let chunks: Vec<String> = result
        .text_stream()
        .map(|chunk| chunk.unwrap())
        .collect()
        .await;
    assert_eq!(chunks, vec!["a", "b", "c"]);
}

#[derive(Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct LookupOrder {
    order_id: String,
}

#[tokio::test]
async fn tools_and_stop_conditions_are_available_from_the_prelude() {
    let tools = ToolSet::new()
        .insert(
            "lookup_order",
            Tool::function::<LookupOrder>()
                .description("Look up an order by id.")
                .execute(|input: LookupOrder, _ctx: ToolContext| async move {
                    Ok::<_, ToolError>(json!({ "order_id": input.order_id, "status": "shipped" }))
                })
                .build(),
        )
        .unwrap();
    let model = MockLanguageModel::builder()
        .generate(text_result("done"))
        .build_shared();
    let result = generate_text(model)
        .prompt("Where is order 4521?")
        .tools(tools)
        .stop_when(step_count(5))
        .await
        .unwrap();
    assert_eq!(result.text(), "done");
    let history: Vec<Message> = vec![Message::user("hi")];
    assert_eq!(history.len(), 1);
}
