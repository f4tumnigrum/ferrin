use std::sync::Arc;

use chrono::TimeZone;
use chrono::Utc;
use ferrin_core::Error;
use ferrin_core::Output;
use ferrin_core::StepContent;
use ferrin_core::clock::FixedClock;
use ferrin_core::generate_text;
use ferrin_core::generate_text::Include;
use ferrin_core::step_count;
use ferrin_message::Message;
use ferrin_schema::schemars;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::language_model::prompt::PromptMessage;
use ferrin_spec::language_model::prompt::ToolPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::UserPromptPart;
use ferrin_testing::SequentialIdGenerator;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::json;

use super::common::kinds;
use super::common::mock;
use super::common::text_model;
use super::common::text_result;
use super::common::tool_call_result;
use super::common::weather_tools;

#[tokio::test]
async fn generates_text_and_records_the_prompt() {
    let model = text_model("Hello!");
    let result = generate_text(Arc::clone(&model))
        .system("Be brief.")
        .prompt("Say hi")
        .temperature(0.5)
        .max_output_tokens(64)
        .await
        .unwrap();

    assert_eq!(result.text(), "Hello!");
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.finish_reason().unified.to_string(), "stop");
    assert_eq!(result.total_usage.total_tokens(), Some(15));
    assert_eq!(result.output, ());

    let calls = model.generate_calls();
    assert_eq!(calls.len(), 1);
    let options = &calls[0];
    assert_eq!(options.temperature, Some(0.5));
    assert_eq!(options.max_output_tokens, Some(64));
    assert_eq!(options.prompt.len(), 2);
    assert!(matches!(
        &options.prompt[0],
        PromptMessage::System { content, .. } if content == "Be brief."
    ));
    match &options.prompt[1] {
        PromptMessage::User { content, .. } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], UserPromptPart::Text(part) if part.text == "Say hi"));
        }
        other => panic!("unexpected message {other:?}"),
    }
}

#[tokio::test]
async fn fills_response_metadata_from_clock_and_id_generator() {
    let model = text_model("x");
    let instant = Utc.with_ymd_and_hms(2026, 9, 13, 12, 0, 0).unwrap();
    let result = generate_text(Arc::clone(&model))
        .prompt("hi")
        .clock(Arc::new(FixedClock(instant)))
        .id_generator(Arc::new(SequentialIdGenerator::new("id")))
        .await
        .unwrap();
    let step = result.last_step();
    assert_eq!(step.response.timestamp, Some(instant));
    assert!(step.response.id.as_deref().unwrap().starts_with("id-"));
    assert_eq!(
        step.response.model_id.as_ref().unwrap().as_str(),
        "mock-model"
    );
    assert_eq!(step.model.provider.as_str(), "mock");
    assert_eq!(step.model.model_id.as_str(), "mock-model");
}

#[tokio::test]
async fn rejects_missing_or_conflicting_prompts() {
    let error = generate_text(text_model("x")).await.unwrap_err();
    assert!(matches!(error, Error::InvalidPrompt { .. }), "{error:?}");

    let error = generate_text(text_model("x"))
        .prompt("a")
        .messages([Message::user("b")])
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidPrompt { .. }), "{error:?}");
}

#[tokio::test]
async fn runs_the_tool_loop_and_feeds_results_back() {
    let model = mock()
        .generate(tool_call_result(
            "call-1",
            "get_weather",
            &json!({ "city": "Berlin" }),
        ))
        .generate(text_result("It is 21 degrees in Berlin."))
        .build_shared();

    let result = generate_text(Arc::clone(&model))
        .prompt("Weather in Berlin?")
        .tools(weather_tools())
        .stop_when(step_count(5))
        .include(Include::all())
        .await
        .unwrap();

    assert_eq!(result.steps.len(), 2);
    assert_eq!(result.text(), "It is 21 degrees in Berlin.");
    assert_eq!(result.total_usage.total_tokens(), Some(43));

    let first = &result.steps[0];
    assert_eq!(kinds(&first.content), vec!["tool-call", "tool-result"]);
    match &first.content[1] {
        StepContent::ToolResult(tool_result) => {
            assert_eq!(tool_result.tool_call_id.as_str(), "call-1");
            assert_eq!(tool_result.tool_name.as_str(), "get_weather");
            assert_eq!(tool_result.input, json!({ "city": "Berlin" }));
            assert_eq!(
                tool_result.output,
                json!({ "city": "Berlin", "temperature": 21 })
            );
            assert!(!tool_result.preliminary);
            assert!(tool_result.execution_ms.is_some());
        }
        other => panic!("unexpected content {other:?}"),
    }
    assert_eq!(first.response.messages.len(), 2);
    assert_eq!(first.response.messages[0].role().as_str(), "assistant");
    assert_eq!(first.response.messages[1].role().as_str(), "tool");

    let calls = model.generate_calls();
    assert_eq!(calls.len(), 2);
    let first_call = &calls[0];
    assert_eq!(first_call.tools.len(), 1);
    match &first_call.tools[0] {
        ToolDefinition::Function {
            name, description, ..
        } => {
            assert_eq!(name.as_str(), "get_weather");
            assert_eq!(description.as_deref(), Some("Get the weather for a city."));
        }
        other => panic!("unexpected tool definition {other:?}"),
    }
    let second_call = &calls[1];
    assert_eq!(second_call.prompt.len(), 3);
    assert!(matches!(
        second_call.prompt[1],
        PromptMessage::Assistant { .. }
    ));
    match &second_call.prompt[2] {
        PromptMessage::Tool { content, .. } => match &content[0] {
            ToolPromptPart::ToolResult(part) => {
                assert_eq!(part.tool_call_id.as_str(), "call-1");
                assert_eq!(
                    part.output,
                    ToolResultOutput::json(json!({ "city": "Berlin", "temperature": 21 }))
                );
            }
            other => panic!("unexpected part {other:?}"),
        },
        other => panic!("unexpected message {other:?}"),
    }

    assert_eq!(result.response_messages().len(), 3);
    assert_eq!(result.steps[1].request.messages.as_ref().unwrap().len(), 3);
}

#[tokio::test]
async fn stop_conditions_end_the_loop_after_tool_execution() {
    let model = mock()
        .generate_repeat(tool_call_result(
            "call-1",
            "get_weather",
            &json!({ "city": "Paris" }),
        ))
        .build_shared();
    let result = generate_text(Arc::clone(&model))
        .prompt("hi")
        .tools(weather_tools())
        .stop_when(step_count(1))
        .await
        .unwrap();
    assert_eq!(result.steps.len(), 1);
    assert_eq!(
        kinds(&result.steps[0].content),
        vec!["tool-call", "tool-result"]
    );
    assert_eq!(result.finish_reason().unified.to_string(), "tool-calls");
    assert_eq!(model.call_count(), 1);
}

#[tokio::test]
async fn invalid_tool_input_and_unknown_tools_become_tool_errors() {
    let model = mock()
        .generate(tool_call_result(
            "call-1",
            "get_weather",
            &json!({ "city": 5 }),
        ))
        .generate(tool_call_result("call-2", "nope", &json!({})))
        .generate(text_result("done"))
        .build_shared();
    let result = generate_text(Arc::clone(&model))
        .prompt("hi")
        .tools(weather_tools())
        .stop_when(step_count(5))
        .await
        .unwrap();

    assert_eq!(result.steps.len(), 3);
    let step = &result.steps[0];
    assert_eq!(kinds(&step.content), vec!["tool-call", "tool-error"]);
    match (&step.content[0], &step.content[1]) {
        (StepContent::ToolCall(call), StepContent::ToolError(error)) => {
            assert!(call.invalid);
            assert!(call.dynamic);
            assert!(call.error.is_some());
            assert_eq!(error.tool_call_id.as_str(), "call-1");
            assert!(error.dynamic);
        }
        other => panic!("unexpected content {other:?}"),
    }
    let step = &result.steps[1];
    assert_eq!(kinds(&step.content), vec!["tool-call", "tool-error"]);
    match &step.content[1] {
        StepContent::ToolError(error) => {
            assert_eq!(error.tool_name.as_str(), "nope");
            assert!(error.error.message().contains("nope"));
        }
        other => panic!("unexpected content {other:?}"),
    }

    let calls = model.generate_calls();
    match &calls[1].prompt[2] {
        PromptMessage::Tool { content, .. } => match &content[0] {
            ToolPromptPart::ToolResult(part) => assert!(part.output.is_error()),
            other => panic!("unexpected part {other:?}"),
        },
        other => panic!("unexpected message {other:?}"),
    }
}

#[tokio::test]
async fn tool_choice_violation_is_recorded_as_invalid_call() {
    let model = mock()
        .generate(tool_call_result(
            "call-1",
            "get_weather",
            &json!({ "city": "Rome" }),
        ))
        .generate(text_result("ok"))
        .build_shared();
    let tools = weather_tools()
        .insert(
            "other",
            ferrin_tool::Tool::function_with_schema(ferrin_tool::Schema::empty_object()).build(),
        )
        .unwrap();
    let result = generate_text(Arc::clone(&model))
        .prompt("hi")
        .tools(tools)
        .tool_choice(ToolChoice::tool("other"))
        .stop_when(step_count(5))
        .await
        .unwrap();
    match &result.steps[0].content[0] {
        StepContent::ToolCall(call) => {
            assert!(call.invalid);
            assert!(call.error.as_deref().unwrap().contains("other"));
        }
        other => panic!("unexpected content {other:?}"),
    }
}

#[derive(Debug, Deserialize, PartialEq, schemars::JsonSchema)]
struct Weather {
    city: String,
    temperature: i64,
}

#[tokio::test]
async fn structured_output_is_parsed_and_requested_as_json() {
    let model = text_model("{\"city\":\"Oslo\",\"temperature\":3}");
    let result = generate_text(Arc::clone(&model))
        .prompt("hi")
        .output(Output::<Weather>::object())
        .await
        .unwrap();
    assert_eq!(
        result.output,
        Weather {
            city: "Oslo".to_owned(),
            temperature: 3,
        }
    );
    let options = &model.generate_calls()[0];
    let format = options.response_format.as_ref().unwrap();
    assert!(
        format!("{format:?}").contains("temperature"),
        "schema must be sent: {format:?}"
    );

    let error = generate_text(text_model("not json"))
        .prompt("hi")
        .output(Output::<Weather>::object())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NoObjectGenerated(_)), "{error:?}");
}

#[tokio::test]
async fn array_and_choice_outputs() {
    let result = generate_text(text_model(
        "{\"elements\":[{\"city\":\"A\",\"temperature\":1}]}",
    ))
    .prompt("hi")
    .output(Output::<Vec<Weather>>::array())
    .await
    .unwrap();
    assert_eq!(result.output.len(), 1);
    assert_eq!(result.output[0].city, "A");

    let result = generate_text(text_model("{\"result\":\"yes\"}"))
        .prompt("hi")
        .output(Output::choice(["yes", "no"]))
        .await
        .unwrap();
    assert_eq!(result.output, "yes");
}
