//! Regressions against ai-sdk 6c6c221 output strategy behavior.

use std::sync::Arc;

use ferrin_core::Error;
use ferrin_core::Output;
use ferrin_core::generate_text;
use ferrin_core::output::ArrayOutput;
use ferrin_core::output::OutputContext;
use ferrin_core::stream_text;
use ferrin_schema::Schema;
use ferrin_schema::schemars;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonValue;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Usage;
use ferrin_testing::text_parts;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::json;

use super::common::mock;

#[derive(Debug, PartialEq, Deserialize, schemars::JsonSchema)]
struct Item {
    value: u64,
}

fn context() -> OutputContext {
    OutputContext {
        response: ResponseMetadata::default(),
        usage: Usage::default(),
        finish_reason: FinishReason::stop(),
    }
}

#[test]
fn array_partial_retains_valid_elements_after_an_invalid_completed_element() {
    let output = Output::<Vec<Item>>::array().handler();
    assert_eq!(
        output.parse_partial(r#"{"elements":[{"value":"bad"},{"value":2},{"value":3"#),
        Some(json!([{"value":2}]))
    );
    assert_eq!(
        output.parse_partial(r#"{"elements":[{"value":"bad"},{"value":2}]}"#),
        Some(json!([{"value":2}]))
    );
    assert!(
        output
            .parse_complete(r#"{"elements":[{"value":"bad"},{"value":2}]}"#, &context())
            .is_err()
    );
}

#[tokio::test]
async fn array_element_stream_applies_custom_schema_validation() {
    let schema = Schema::with_json_schema_and_validator(
        json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"]}),
        |value: JsonValue| {
            Ok(Item {
                value: value["value"].as_u64().unwrap() + 10,
            })
        },
    );
    let model = mock()
        .stream(text_parts(
            [r#"{"elements":[{"value":1},{"value":2}]}"#],
            Usage::default(),
        ))
        .build_shared();
    let mut stream = stream_text(model)
        .prompt("hi")
        .output(Output::array_with(schema))
        .await
        .unwrap();
    let elements = stream.element_view();
    let result = stream.final_result().await.unwrap();
    let elements: Vec<_> = elements.map(Result::unwrap).collect().await;
    assert_eq!(elements, vec![Item { value: 11 }, Item { value: 12 }]);
    assert_eq!(elements, result.output);
}

#[tokio::test]
async fn array_element_stream_rejects_overflow_before_publishing_it() {
    let model = mock()
        .stream(text_parts(
            [r#"{"elements":[{"value":1},{"value":2}]}"#],
            Usage::default(),
        ))
        .build_shared();
    let stream = stream_text(model)
        .prompt("hi")
        .output(Output::custom(
            ArrayOutput::new(Schema::<Item>::derived()).max_items(1),
        ))
        .await
        .unwrap();
    let values: Vec<_> = stream.element_stream().collect().await;
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].as_ref().unwrap(), &Item { value: 1 });
    assert!(matches!(
        values[1].as_ref().unwrap_err().as_provider(),
        Some(ferrin_spec::error::ProviderError::TypeValidation(_))
    ));
}

#[tokio::test]
async fn inconsistent_bounds_fail_before_generate_or_stream_requests() {
    let model = mock().build_shared();
    let output = Output::custom(
        ArrayOutput::new(Schema::<Item>::derived())
            .min_items(2)
            .max_items(1),
    );
    assert!(matches!(
        generate_text(Arc::clone(&model))
            .prompt("hi")
            .output(output.clone())
            .await,
        Err(Error::InvalidArgument { .. })
    ));
    assert!(matches!(
        stream_text(Arc::clone(&model))
            .prompt("hi")
            .output(output)
            .await,
        Err(Error::InvalidArgument { .. })
    ));
    assert_eq!(model.call_count(), 0);
}

#[test]
fn choice_partials_require_unique_prefix_or_exact_complete_value() {
    let output = Output::choice(["blue", "black", "red"]).handler();
    assert_eq!(
        [
            output.parse_partial(r#"{"result":"bl"#),
            output.parse_partial(r#"{"result":"blu"#),
            output.parse_partial(r#"{"result":"blu"}"#),
            output.parse_partial(r#"{"result":"blue"}"#),
        ],
        [None, Some(json!("blue")), None, Some(json!("blue"))]
    );
    assert!(
        output
            .parse_complete(r#"{"result":"green"}"#, &context())
            .is_err()
    );
}

#[test]
fn json_typed_partials_honor_schema_and_unconstrained_json_accepts_every_kind() {
    let constrained = Output::json_with_schema(
        json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"]}),
    )
    .handler();
    assert_eq!(
        constrained.parse_partial(r#"{"value":"preview"}"#),
        Some(json!({"value":"preview"}))
    );
    assert_eq!(constrained.typed_partial(&json!({"value":"preview"})), None);
    assert_eq!(
        constrained.typed_partial(&json!({"value":1})),
        Some(json!({"value":1}))
    );
    let output = Output::json().handler();
    for value in [
        json!(null),
        json!(true),
        json!(3),
        json!("text"),
        json!([1]),
        json!({"value":1}),
    ] {
        assert_eq!(
            output
                .parse_complete(&value.to_string(), &context())
                .unwrap(),
            value
        );
    }
}

#[tokio::test]
async fn partial_output_keeps_the_first_text_part_across_step_boundaries() {
    let mut first = text_parts([r#"{"phase":1}"#], Usage::default());
    first.pop();
    first.push(ferrin_spec::StreamPart::ToolCall(
        ferrin_spec::ToolCall::new("weather-1", "get_weather", r#"{"city":"Oslo"}"#),
    ));
    first.push(ferrin_spec::StreamPart::finish(
        FinishReason::tool_calls(),
        Usage::default(),
    ));
    let model = mock()
        .stream(first)
        .stream(text_parts([r#"{"phase":2}"#], Usage::default()))
        .build_shared();
    let mut stream = stream_text(model)
        .prompt("hi")
        .tools(super::common::weather_tools())
        .stop_when(ferrin_core::step_count(2))
        .output(Output::json())
        .await
        .unwrap();
    let partial = stream.partial_output_view();
    let result = stream.final_result().await.unwrap();
    let partial: Vec<_> = partial.map(|output| output.value).collect().await;
    assert_eq!(
        (partial, result.output),
        (vec![json!({"phase":1})], json!({"phase":2}))
    );
}

#[tokio::test]
async fn array_element_retry_preserves_the_already_published_prefix_count() {
    let mut failed = text_parts([r#"{"elements":[{"value":1}]}"#], Usage::default());
    failed.pop();
    failed.push(ferrin_spec::StreamPart::Error {
        error: ferrin_spec::language_model::StreamError::new("retry"),
    });
    let model = mock()
        .stream(failed)
        .stream(text_parts(
            [r#"{"elements":[{"value":2},{"value":3}]}"#],
            Usage::default(),
        ))
        .build_shared();
    let mut stream = stream_text(model)
        .prompt("hi")
        .stream_retries(1)
        .output(Output::<Vec<Item>>::array())
        .await
        .unwrap();
    let elements = stream.element_view();
    let result = stream.final_result().await.unwrap();
    let elements: Vec<_> = elements.map(Result::unwrap).collect().await;
    assert_eq!(elements, vec![Item { value: 1 }, Item { value: 3 }]);
    assert_eq!(result.output, vec![Item { value: 2 }, Item { value: 3 }]);
}
