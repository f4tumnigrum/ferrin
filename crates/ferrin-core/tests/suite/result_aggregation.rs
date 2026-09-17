//! Aggregate result accessors preserve generation order across tool steps.

use ferrin_core::StepContent;
use ferrin_core::generate_text;
use ferrin_core::step_count;
use ferrin_spec::Content;
use ferrin_spec::FileData;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::Source;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::text_result;
use super::common::tool_call_result;
use super::common::weather_tools;

#[tokio::test]
async fn result_accessors_aggregate_all_steps_and_keep_final_step_metadata() {
    let mut first = tool_call_result("weather-1", "get_weather", &json!({"city":"Oslo"}));
    first.warnings = vec![Warning::unsupported("first setting")];
    first.content.insert(0, Content::text("Checking"));
    first.content.push(Content::File {
        data: FileData::bytes(b"first".as_slice()),
        media_type: "text/plain".into(),
        filename: None,
        provider_metadata: None,
    });
    let mut second = text_result("Final answer");
    second.warnings = vec![Warning::unsupported("second setting")];
    second.content.push(Content::Source(Source::Url {
        id: "source-1".to_owned(),
        url: "https://example.com".to_owned(),
        title: None,
        provider_metadata: None,
    }));
    let result = generate_text(mock().generate(first).generate(second).build_shared())
        .prompt("Weather")
        .tools(weather_tools())
        .stop_when(step_count(2))
        .await
        .unwrap();
    assert_eq!(result.usage(), &Usage::totals(30, 13));
    assert_eq!(
        result.warnings(),
        vec![
            &Warning::unsupported("first setting"),
            &Warning::unsupported("second setting")
        ]
    );
    assert_eq!(
        result
            .content()
            .map(StepContent::kind_name)
            .collect::<Vec<_>>(),
        vec!["text", "tool-call", "file", "tool-result", "text", "source"]
    );
    assert_eq!(result.files().count(), 1);
    assert_eq!(result.sources().count(), 1);
    assert_eq!(
        result
            .tool_calls()
            .map(|call| call.tool_call_id.as_str())
            .collect::<Vec<_>>(),
        vec!["weather-1"]
    );
    assert_eq!(result.static_tool_calls().count(), 1);
    assert_eq!(result.dynamic_tool_calls().count(), 0);
    assert_eq!(
        result
            .tool_results()
            .map(|value| &value.output)
            .collect::<Vec<_>>(),
        vec![&json!({"city":"Oslo","temperature":21})]
    );
    assert_eq!(result.static_tool_results().count(), 1);
    assert_eq!(result.dynamic_tool_results().count(), 0);
    assert_eq!(result.text(), "Final answer");
    assert_eq!(
        (result.final_step(), result.request(), result.response()),
        (
            result.last_step(),
            &result.last_step().request,
            &result.last_step().response
        )
    );
    assert_eq!(
        result.raw_finish_reason(),
        result.last_step().finish_reason.raw.as_deref()
    );
}
