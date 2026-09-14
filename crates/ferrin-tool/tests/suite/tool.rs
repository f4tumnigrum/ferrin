use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use ferrin_schema::schemars;
use ferrin_spec::ToolDefinition;
use ferrin_spec::ToolName;
use ferrin_tool::DescriptionContext;
use ferrin_tool::NeedsApproval;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolKind;
use ferrin_tool::ToolOutput;
use ferrin_tool::execute_to_completion;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "ferrin_schema::schemars")]
struct GetWeatherInput {
    /// City name.
    city: String,
}

#[derive(Debug, Serialize)]
struct Weather {
    temperature_c: f32,
    condition: String,
}

fn weather_tool() -> Tool {
    Tool::function::<GetWeatherInput>()
        .description("Get the current weather for a city.")
        .execute(|input: GetWeatherInput, _ctx| async move {
            Ok(Weather {
                temperature_c: 21.5,
                condition: format!("sunny in {}", input.city),
            })
        })
        .build()
}

#[tokio::test]
async fn function_tool_derives_schema_and_executes() {
    let tool = weather_tool();
    assert_eq!(tool.kind(), &ToolKind::Function);
    let schema = tool.input_schema().json_schema();
    assert_eq!(schema["type"], json!("object"));
    assert_eq!(schema["properties"]["city"]["type"], json!("string"));
    assert_eq!(
        schema["properties"]["city"]["description"],
        json!("City name.")
    );
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(schema["required"], json!(["city"]));

    let name = ToolName::new("get_weather");
    let input = tool
        .validate_input(&name, json!({ "city": "Berlin" }))
        .unwrap();
    let stream = tool.execute(input, ToolContext::new("call_1")).unwrap();
    let output = execute_to_completion(stream, |_| panic!("no preliminary outputs"))
        .await
        .unwrap();
    assert_eq!(
        output,
        json!({ "temperature_c": 21.5, "condition": "sunny in Berlin" })
    );

    let error = tool
        .validate_input(&name, json!({ "city": 3 }))
        .unwrap_err();
    assert!(error.to_string().contains("tool input"), "{error}");
    assert!(error.to_string().contains("get_weather"), "{error}");
}

#[tokio::test]
async fn definition_reflects_kind() {
    let tool = weather_tool();
    let description = tool
        .resolve_description(DescriptionContext::default())
        .await;
    assert_eq!(
        description.as_deref(),
        Some("Get the current weather for a city.")
    );
    match tool.definition("get_weather".into(), description) {
        ToolDefinition::Function {
            name,
            description,
            strict,
            ..
        } => {
            assert_eq!(name.as_str(), "get_weather");
            assert_eq!(
                description.as_deref(),
                Some("Get the current weather for a city.")
            );
            assert_eq!(strict, None);
        }
        other => panic!("unexpected definition {other:?}"),
    }

    let provider = Tool::provider_executed(
        "openai.web_search",
        json!({ "max_uses": 2 }).as_object().unwrap().clone(),
    )
    .supports_deferred_results(true)
    .build();
    assert!(provider.kind().is_provider_executed());
    assert_eq!(provider.kind().provider_id(), Some("openai.web_search"));
    assert!(matches!(
        provider.kind(),
        ToolKind::ProviderExecuted {
            supports_deferred_results: true,
            ..
        }
    ));
    match provider.definition("search".into(), None) {
        ToolDefinition::Provider { id, name, args } => {
            assert_eq!(id, "openai.web_search");
            assert_eq!(name.as_str(), "search");
            assert_eq!(args["max_uses"], json!(2));
        }
        other => panic!("unexpected definition {other:?}"),
    }
}

#[tokio::test]
async fn dynamic_description_and_approval() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let tool = Tool::function_with_schema(Schema::empty_object())
        .description_fn(move |ctx: DescriptionContext| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                format!("context: {}", ctx.tool_context.unwrap_or_default())
            }
        })
        .needs_approval_if(|input, _ctx| async move { input["danger"] == json!(true) })
        .build();
    let description = tool
        .resolve_description(DescriptionContext::with_tool_context(json!("x")))
        .await;
    assert_eq!(description.as_deref(), Some("context: \"x\""));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(tool.needs_approval().is_declared());
    assert!(
        tool.needs_approval()
            .resolve(json!({ "danger": true }), ToolContext::new("c"))
            .await
    );
    assert!(
        !tool
            .needs_approval()
            .resolve(json!({ "danger": false }), ToolContext::new("c"))
            .await
    );
    assert!(
        !NeedsApproval::Never
            .resolve(json!({}), ToolContext::new("c"))
            .await
    );
    assert!(
        NeedsApproval::Always
            .resolve(json!({}), ToolContext::new("c"))
            .await
    );
}

#[tokio::test]
async fn stream_execution_emits_preliminary_then_final() {
    let tool = Tool::dynamic(Schema::any())
        .execute_stream(|_input: serde_json::Value, _ctx| {
            futures_util::stream::iter(vec![Ok(json!("a")), Ok(json!("b"))])
        })
        .build();
    assert!(tool.kind().is_dynamic());
    let outputs: Vec<_> = tool
        .execute(json!({}), ToolContext::new("call"))
        .unwrap()
        .map(Result::unwrap)
        .collect()
        .await;
    assert_eq!(
        outputs,
        vec![
            ToolOutput::Preliminary(json!("a")),
            ToolOutput::Preliminary(json!("b")),
            ToolOutput::Final(json!("b")),
        ]
    );

    let failing = Tool::dynamic(Schema::any())
        .execute_stream(|_input: serde_json::Value, _ctx| {
            futures_util::stream::iter(vec![Ok(json!(1)), Err(ToolError::message("boom"))])
        })
        .build();
    let stream = failing
        .execute(json!({}), ToolContext::new("call"))
        .unwrap();
    let mut seen = Vec::new();
    let error = execute_to_completion(stream, |value| seen.push(value))
        .await
        .unwrap_err();
    assert_eq!(seen, vec![json!(1)]);
    assert_eq!(error.to_string(), "boom");
}

#[tokio::test]
async fn invalid_input_for_typed_closure_is_a_tool_error() {
    let tool = weather_tool();
    let stream = tool
        .execute(json!({ "city": 5 }), ToolContext::new("call"))
        .unwrap();
    let error = execute_to_completion(stream, |_| {}).await.unwrap_err();
    assert!(
        error.to_string().starts_with("invalid tool input"),
        "{error}"
    );
    assert!(tool.is_executable());
    let passive = Tool::function::<GetWeatherInput>().build();
    assert!(!passive.is_executable());
    assert!(
        passive
            .execute(json!({}), ToolContext::new("call"))
            .is_none()
    );
}

#[test]
fn context_validation() {
    let name = ToolName::new("t");
    let without = Tool::function::<GetWeatherInput>().build();
    assert_eq!(
        without
            .validate_context(&name, Some(json!({ "user": 1 })))
            .unwrap(),
        None
    );
    let with = Tool::function::<GetWeatherInput>()
        .context_schema(Schema::from_json_schema(json!({
            "type": "object",
            "properties": { "user": { "type": "string" } },
            "required": ["user"]
        })))
        .build();
    assert_eq!(
        with.validate_context(&name, Some(json!({ "user": "u1" })))
            .unwrap(),
        Some(json!({ "user": "u1" }))
    );
    let error = with.validate_context(&name, None).unwrap_err();
    assert!(error.to_string().contains("tool context"), "{error}");
}

#[test]
fn tool_error_helpers() {
    let error = ToolError::message("bad").with_cause(std::fmt::Error);
    assert!(matches!(error, ToolError::Message { cause: Some(_), .. }));
    assert!(ToolError::Cancelled.is_cancelled());
    let json_error: ToolError = serde_json::from_str::<u8>("x").unwrap_err().into();
    assert!(matches!(json_error, ToolError::Message { .. }));
}
