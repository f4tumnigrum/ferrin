use std::collections::HashMap;

use ferrin_spec::ProviderOptions;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolCaller;
use ferrin_tool::ToolCallerDefinition;
use ferrin_tool::ToolCallers;
use ferrin_tool::ToolSet;
use ferrin_tool::callers::prepare_tools_for_callers;
use ferrin_tool::callers::validate_tool_callers;
use pretty_assertions::assert_eq;
use serde_json::json;

fn plain() -> Tool {
    Tool::function_with_schema(Schema::empty_object()).build()
}

fn callers(entries: Vec<(&str, Vec<ToolCaller>)>) -> ToolCallers {
    entries
        .into_iter()
        .map(|(name, list)| (name.into(), list))
        .collect::<HashMap<_, _>>()
}

#[test]
fn validation_rejects_unknown_tools_and_callers() {
    let tools = ToolSet::new()
        .insert("a", plain())
        .unwrap()
        .insert("runner", plain())
        .unwrap();
    let error = validate_tool_callers(
        &tools,
        &callers(vec![("missing", vec![ToolCaller::Direct])]),
    )
    .unwrap_err();
    assert_eq!(error.argument, "tool_callers");
    assert!(error.message.contains("unknown tool"));
    let error = validate_tool_callers(
        &tools,
        &callers(vec![("a", vec![ToolCaller::Tool("runner".into())])]),
    )
    .unwrap_err();
    assert!(error.message.contains("invalid caller"));
    assert!(validate_tool_callers(&tools, &callers(vec![("a", vec![ToolCaller::Direct])])).is_ok());
}

#[test]
fn local_callers_receive_callees_and_hide_them_from_the_model() {
    let runner = Tool::function_with_schema(Schema::empty_object())
        .caller(ToolCallerDefinition::local(|callees: ToolSet| {
            let names: Vec<String> = callees.names().map(ToString::to_string).collect();
            Tool::function_with_schema(Schema::empty_object())
                .description(format!("runs {}", names.join(",")))
                .build()
        }))
        .build();
    let tools = ToolSet::new()
        .insert("runner", runner)
        .unwrap()
        .insert("helper", plain())
        .unwrap()
        .insert("shared", plain())
        .unwrap();
    let config = callers(vec![
        ("helper", vec![ToolCaller::Tool("runner".into())]),
        (
            "shared",
            vec![ToolCaller::Direct, ToolCaller::Tool("runner".into())],
        ),
    ]);
    validate_tool_callers(&tools, &config).unwrap();
    let prepared = prepare_tools_for_callers(&tools, &config);

    let model_names: Vec<&str> = prepared
        .model_tools
        .names()
        .map(ferrin_spec::ToolName::as_str)
        .collect();
    assert_eq!(model_names, vec!["runner", "shared"]);
    let execution_names: Vec<&str> = prepared
        .execution_tools
        .names()
        .map(ferrin_spec::ToolName::as_str)
        .collect();
    assert_eq!(execution_names, vec!["runner", "helper", "shared"]);
    let bound = prepared.execution_tools.get("runner").unwrap();
    assert_eq!(
        bound.description().and_then(|d| d.as_static()),
        Some("runs helper,shared")
    );
    assert_eq!(
        prepared
            .model_tools
            .get("runner")
            .unwrap()
            .description()
            .and_then(|d| d.as_static()),
        Some("runs helper,shared")
    );
}

#[test]
fn provider_callers_prepare_options() {
    let caller = Tool::provider_executed("acme.orchestrator", Default::default())
        .caller(ToolCallerDefinition::provider(
            |options: Option<ProviderOptions>| {
                let mut options = options.unwrap_or_default();
                options
                    .entry("acme".to_owned())
                    .or_default()
                    .insert("allowed_caller".to_owned(), json!("orchestrator"));
                options
            },
        ))
        .build();
    let tools = ToolSet::new()
        .insert("orchestrator", caller)
        .unwrap()
        .insert("worker", plain())
        .unwrap();
    let config = callers(vec![(
        "worker",
        vec![ToolCaller::Tool("orchestrator".into())],
    )]);
    let prepared = prepare_tools_for_callers(&tools, &config);
    let worker = prepared.model_tools.get("worker").unwrap();
    assert_eq!(
        worker.provider_options().unwrap()["acme"]["allowed_caller"],
        json!("orchestrator")
    );
    assert!(
        prepared
            .execution_tools
            .get("worker")
            .unwrap()
            .provider_options()
            .is_some()
    );
    assert!(tools.get("worker").unwrap().provider_options().is_none());
}
