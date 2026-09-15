use std::sync::Arc;

use ferrin_core::generate_text;
use ferrin_core::generate_text::PrepareStepContext;
use ferrin_core::generate_text::StepOverrides;
use ferrin_core::step_count;
use ferrin_message::Message;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::language_model::prompt::PromptMessage;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::text_result;
use super::common::tool_call_result;
use super::common::weather_tools;

fn tool_names(tools: &[ToolDefinition]) -> Vec<&str> {
    tools
        .iter()
        .map(|tool| match tool {
            ToolDefinition::Function { name, .. } | ToolDefinition::Provider { name, .. } => {
                name.as_str()
            }
            _ => "?",
        })
        .collect()
}

#[tokio::test]
async fn overrides_apply_per_step() {
    let model = mock()
        .generate(tool_call_result(
            "call-1",
            "get_weather",
            &json!({ "city": "Bern" }),
        ))
        .generate(text_result("done"))
        .build_shared();
    let tools = weather_tools()
        .insert(
            "other",
            Tool::function_with_schema(Schema::empty_object()).build(),
        )
        .unwrap();
    let result = generate_text(Arc::clone(&model))
        .prompt("hi")
        .tools(tools)
        .stop_when(step_count(5))
        .prepare_step(|ctx: &PrepareStepContext<'_>| {
            if ctx.step_number == 0 {
                StepOverrides::none()
                    .with_tool_choice(ToolChoice::Required)
                    .with_active_tools(["get_weather"])
                    .with_instructions("Step zero.")
            } else {
                StepOverrides::none()
                    .with_tool_choice(ToolChoice::None)
                    .with_tool_order(["other", "get_weather"])
            }
        })
        .await
        .unwrap();
    assert_eq!(result.steps.len(), 2);

    let calls = model.generate_calls();
    assert_eq!(tool_names(&calls[0].tools), vec!["get_weather"]);
    assert_eq!(calls[0].tool_choice, Some(ToolChoice::Required));
    assert!(matches!(
        &calls[0].prompt[0],
        PromptMessage::System { content, .. } if content == "Step zero."
    ));
    assert_eq!(tool_names(&calls[1].tools), vec!["other", "get_weather"]);
    assert_eq!(calls[1].tool_choice, Some(ToolChoice::None));
    assert!(!matches!(&calls[1].prompt[0], PromptMessage::System { .. }));
}

#[tokio::test]
async fn message_overrides_replace_the_history_sent_to_the_model() {
    let model = mock().generate(text_result("ok")).build_shared();
    generate_text(Arc::clone(&model))
        .messages([
            Message::user("original"),
            Message::assistant("reply"),
            Message::user("more"),
        ])
        .prepare_step(|ctx: &PrepareStepContext<'_>| {
            assert_eq!(ctx.messages.len(), 3);
            StepOverrides::none().with_messages([Message::user("trimmed")])
        })
        .await
        .unwrap();
    let calls = model.generate_calls();
    assert_eq!(calls[0].prompt.len(), 1);
    assert!(matches!(&calls[0].prompt[0], PromptMessage::User { .. }));
}

#[tokio::test]
async fn empty_effective_tools_clear_required_choice_in_both_loops() {
    for choice in [ToolChoice::Required, ToolChoice::tool("get_weather")] {
        let model = mock()
            .generate(text_result("done"))
            .stream(ferrin_testing::text_parts(
                ["done"],
                ferrin_spec::Usage::default(),
            ))
            .build_shared();
        let generated = generate_text(Arc::clone(&model))
            .prompt("hi")
            .tools(weather_tools())
            .tool_choice(choice.clone())
            .prepare_step(|_: &PrepareStepContext<'_>| {
                StepOverrides::none().with_active_tools(Vec::<ferrin_spec::ToolName>::new())
            })
            .await
            .unwrap();
        let streamed = ferrin_core::stream_text(Arc::clone(&model))
            .prompt("hi")
            .tools(weather_tools())
            .tool_choice(choice)
            .prepare_step(|_: &PrepareStepContext<'_>| {
                StepOverrides::none().with_active_tools(Vec::<ferrin_spec::ToolName>::new())
            })
            .await
            .unwrap()
            .consume()
            .await
            .unwrap();
        assert_eq!(
            (generated.text(), streamed.text()),
            ("done".into(), "done".into())
        );
        for call in model
            .generate_calls()
            .iter()
            .chain(model.stream_calls().iter())
        {
            assert_eq!((&call.tools, &call.tool_choice), (&Vec::new(), &None));
        }
    }
}
