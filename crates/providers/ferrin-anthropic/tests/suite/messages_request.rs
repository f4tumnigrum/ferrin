//! Request body assembly of the Messages API: snapshots and warnings.

use ferrin_anthropic::request::prepare_request;
use ferrin_anthropic::tools::AnthropicTools;
use ferrin_spec::CallOptions;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::ReasoningEffort;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolDefinition;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::anthropic_options;
use super::common::options_under;

fn features(warnings: &[Warning]) -> Vec<&str> {
    warnings
        .iter()
        .map(|warning| match warning {
            Warning::Unsupported { feature, .. } | Warning::Compatibility { feature, .. } => {
                feature.as_str()
            }
            Warning::Deprecated { setting, .. } => setting.as_str(),
            Warning::Other { message } => message.as_str(),
            _ => "?",
        })
        .collect()
}

fn prepare(
    test: &TestProvider,
    model: &str,
    options: &CallOptions,
) -> ferrin_anthropic::request::PreparedRequest {
    prepare_request(
        test.provider.config(),
        model,
        options,
        false,
        std::collections::BTreeSet::new(),
    )
    .unwrap()
}

fn betas(prepared: &ferrin_anthropic::request::PreparedRequest) -> Vec<String> {
    prepared.betas.iter().cloned().collect()
}

#[tokio::test]
async fn basic_request_snapshot() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![
        PromptMessage::system("Be brief."),
        PromptMessage::user_text("Hello"),
        PromptMessage::assistant_text("Hi!  "),
        PromptMessage::user_text("How are you?"),
    ]);
    options.max_output_tokens = Some(100);
    options.temperature = Some(0.3);
    options.top_k = Some(5);
    options.stop_sequences = Some(vec!["END".to_owned()]);
    let prepared = prepare(&test, "claude-sonnet-4-5", &options);
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    assert!(prepared.betas.is_empty());
    assert!(!prepared.uses_json_response_tool);
    insta::assert_json_snapshot!("messages_request_basic", prepared.body);
}

#[tokio::test]
async fn unsupported_settings_produce_warnings_and_are_dropped() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.seed = Some(7);
    options.presence_penalty = Some(0.1);
    options.frequency_penalty = Some(0.2);
    options.temperature = Some(1.5);
    options.top_p = Some(0.9);
    let prepared = prepare(&test, "claude-sonnet-4-5", &options);
    assert_eq!(
        features(&prepared.warnings),
        vec![
            "frequencyPenalty",
            "presencePenalty",
            "seed",
            "temperature",
            "topP"
        ]
    );
    assert_eq!(prepared.body["temperature"], json!(1.0));
    assert!(prepared.body.get("top_p").is_none());
    assert!(prepared.body.get("seed").is_none());
}

#[tokio::test]
async fn reasoning_on_a_budget_model_enables_thinking_and_drops_sampling() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.reasoning = ReasoningEffort::High;
    options.temperature = Some(0.5);
    let prepared = prepare(&test, "claude-sonnet-4-5", &options);
    assert_eq!(features(&prepared.warnings), vec!["temperature"]);
    assert_eq!(
        prepared.body["thinking"],
        json!({"type": "enabled", "budget_tokens": 38400})
    );
    assert_eq!(prepared.body["max_tokens"], json!(64000));
    assert!(prepared.body.get("temperature").is_none());
    assert!(prepared.body.get("output_config").is_none());
}

#[tokio::test]
async fn reasoning_on_an_adaptive_model_uses_effort() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.reasoning = ReasoningEffort::Medium;
    let prepared = prepare(&test, "claude-opus-4-6", &options);
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    assert_eq!(
        prepared.body["thinking"],
        json!({"type": "adaptive", "display": "summarized"})
    );
    assert_eq!(prepared.body["output_config"], json!({"effort": "medium"}));

    options.reasoning = ReasoningEffort::XHigh;
    let prepared = prepare(&test, "claude-opus-4-6", &options);
    assert_eq!(features(&prepared.warnings), vec!["reasoning"]);
    assert_eq!(prepared.body["output_config"], json!({"effort": "max"}));

    options.reasoning = ReasoningEffort::None;
    let prepared = prepare(&test, "claude-opus-4-6", &options);
    assert_eq!(prepared.body["thinking"], json!({"type": "disabled"}));
    assert!(prepared.body.get("output_config").is_none());
}

#[tokio::test]
async fn provider_options_snapshot() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![
        PromptMessage::system("Be brief."),
        PromptMessage::user_text("Hello"),
    ]);
    options.tools = vec![
        AnthropicTools::new()
            .code_execution_20250825()
            .definition("code_execution".into(), None),
    ];
    options.provider_options = anthropic_options(json!({
        "sendReasoning": false,
        "toolStreaming": false,
        "disableParallelToolUse": true,
        "thinking": {"type": "adaptive", "display": "updates"},
        "effort": "high",
        "cacheControl": {"type": "ephemeral", "ttl": "1h"},
        "metadata": {"userId": "user-1"},
        "serviceTier": "auto",
        "inferenceGeo": "us",
        "speed": "fast",
        "taskBudget": {"type": "tokens", "total": 50000, "remaining": 20000},
        "fallbacks": "default",
        "anthropicBeta": ["extra-beta"],
        "mcpServers": [{
            "type": "url",
            "name": "docs",
            "url": "https://mcp.example.test/sse",
            "authorizationToken": "mcp-token",
            "toolConfiguration": {"enabled": true, "allowedTools": ["search"]}
        }],
        "container": {
            "id": "container_1",
            "skills": [
                {"type": "anthropic", "skillId": "pdf", "version": "latest"},
                {"type": "custom", "providerReference": {"anthropic": "skill_abc"}}
            ]
        },
        "contextManagement": {"edits": [
            {
                "type": "clear_tool_uses_20250919",
                "trigger": {"type": "input_tokens", "value": 1000},
                "keep": {"type": "tool_uses", "value": 2},
                "clearAtLeast": {"type": "input_tokens", "value": 100},
                "clearToolInputs": true,
                "excludeTools": ["search"]
            },
            {"type": "clear_thinking_20251015", "keep": "all"},
            {"type": "compact_20260112", "pauseAfterCompaction": true, "instructions": "keep facts"}
        ]}
    }));
    let prepared = prepare(&test, "claude-opus-4-6", &options);
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    assert!(!prepared.used_custom_key);
    insta::assert_json_snapshot!("messages_request_provider_options", prepared.body);
    insta::assert_json_snapshot!("messages_request_provider_options_betas", betas(&prepared));
}

#[tokio::test]
async fn invalid_known_provider_options_are_invalid_arguments() {
    let test = TestProvider::start().await;
    let base = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    for value in [
        json!({"speed": "turbo"}),
        json!({"thinking": {"type": "sometimes"}}),
        json!({"taskBudget": {"type": "tokens", "total": 10}}),
        json!({"cacheControl": {"type": "ephemeral", "ttl": "2h"}}),
    ] {
        let mut options = base.clone();
        options.provider_options = anthropic_options(value.clone());
        let error = prepare_request(
            test.provider.config(),
            "claude-sonnet-4-5",
            &options,
            false,
            std::collections::BTreeSet::new(),
        )
        .unwrap_err();
        assert!(
            matches!(error, ProviderError::InvalidArgument(_)),
            "{value}: {error:?}"
        );
    }
}

#[tokio::test]
async fn json_schema_uses_native_structured_output_on_supported_models() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Answer")]);
    options.response_format = Some(ResponseFormat::Json {
        schema: Some(json!({
            "type": "object",
            "properties": {"answer": {"type": "string", "format": "email"}},
            "required": ["answer"]
        })),
        name: None,
        description: None,
    });
    let prepared = prepare(&test, "claude-sonnet-4-5", &options);
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    assert!(!prepared.uses_json_response_tool);
    assert!(prepared.body.get("tools").is_none());
    assert!(
        prepared.betas.is_empty(),
        "the beta accompanies function tools only"
    );
    insta::assert_json_snapshot!(
        "messages_request_structured_output",
        prepared.body["output_config"]
    );

    options.provider_options = anthropic_options(json!({"structuredOutputMode": "jsonTool"}));
    let prepared = prepare(&test, "claude-sonnet-4-5", &options);
    assert!(prepared.uses_json_response_tool);
    assert!(prepared.betas.is_empty());
    assert_eq!(prepared.body["tools"][0]["name"], json!("json"));
    assert_eq!(
        prepared.body["tool_choice"],
        json!({"type": "any", "disable_parallel_tool_use": true})
    );

    options.provider_options = Default::default();
    options.response_format = Some(ResponseFormat::Json {
        schema: None,
        name: None,
        description: None,
    });
    let prepared = prepare(&test, "claude-sonnet-4-5", &options);
    assert_eq!(features(&prepared.warnings), vec!["responseFormat"]);
    assert!(prepared.body.get("output_config").is_none());
}

#[tokio::test]
async fn max_output_tokens_is_capped_and_unknown_models_warn() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.max_output_tokens = Some(100_000);
    let prepared = prepare(&test, "claude-sonnet-4-5", &options);
    assert_eq!(features(&prepared.warnings), vec!["maxOutputTokens"]);
    assert_eq!(prepared.body["max_tokens"], json!(64000));

    let prepared = prepare(
        &test,
        "my-fine-tune",
        &CallOptions::new(options.prompt.clone()),
    );
    assert_eq!(features(&prepared.warnings), vec!["maxOutputTokens"]);
    assert_eq!(prepared.body["max_tokens"], json!(4096));

    let prepared = prepare(&test, "claude-opus-5", &CallOptions::new(options.prompt));
    assert!(prepared.warnings.is_empty());
    assert_eq!(prepared.body["max_tokens"], json!(128_000));
}

#[tokio::test]
async fn tools_and_tool_choice_are_converted() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Weather?")]);
    let mut tool = ToolDefinition::function(
        "get_weather",
        Some("Weather lookup".to_owned()),
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    );
    if let ToolDefinition::Function {
        strict,
        provider_options,
        ..
    } = &mut tool
    {
        *strict = Some(true);
        *provider_options = Some(anthropic_options(
            json!({"cacheControl": {"type": "ephemeral"}, "deferLoading": true}),
        ));
    }
    options.tools = vec![tool];
    options.tool_choice = Some(ferrin_spec::ToolChoice::Tool {
        tool_name: "get_weather".into(),
    });
    let prepared = prepare(&test, "claude-sonnet-4-5", &options);
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    assert_eq!(
        prepared.body["tools"],
        json!([{
            "name": "get_weather",
            "description": "Weather lookup",
            "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}},
            "cache_control": {"type": "ephemeral"},
            "strict": true,
            "defer_loading": true
        }])
    );
    assert_eq!(
        prepared.body["tool_choice"],
        json!({"type": "tool", "name": "get_weather"})
    );
}

#[tokio::test]
async fn custom_provider_name_reads_options_from_both_keys_and_mirrors_metadata() {
    let test = TestProvider::start_with(|mut settings| {
        settings.name = Some("myclaude".to_owned());
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/messages", "messages", "text-basic");
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    let mut provider_options =
        anthropic_options(json!({"serviceTier": "auto", "effort": "medium"}));
    provider_options.extend(options_under("myclaude", json!({"effort": "low"})));
    options.provider_options = provider_options;
    let prepared = prepare(&test, "claude-opus-4-6", &options);
    assert!(prepared.used_custom_key);
    assert_eq!(prepared.body["service_tier"], json!("auto"));
    assert_eq!(prepared.body["output_config"], json!({"effort": "low"}));

    let result = test
        .provider
        .messages("claude-opus-4-6")
        .do_generate(options)
        .await
        .unwrap();
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["anthropic"]["usage"]["input_tokens"], json!(42));
    assert_eq!(metadata["myclaude"]["usage"]["input_tokens"], json!(42));
}

#[tokio::test]
async fn documented_block_binding_options_use_camel_case() {
    let test = TestProvider::start().await;
    for field in ["prefixMismatchBehavior", "prefix_mismatch_behavior"] {
        let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
        options.provider_options = anthropic_options(json!({
            "thinking": {"blockBinding": {field: "error"}}
        }));
        let prepared = prepare(&test, "claude-opus-4-6", &options);
        assert_eq!(
            prepared.body["thinking"]["block_binding"],
            json!({"prefix_mismatch_behavior": "error"})
        );
        assert!(
            prepared
                .betas
                .contains("thinking-binding-controls-2026-08-01")
        );
    }
}

#[tokio::test]
async fn cache_breakpoint_limit_is_shared_across_prompt_and_tools() {
    use ferrin_spec::language_model::prompt::TextPart;
    use ferrin_spec::language_model::prompt::UserPromptPart;

    let test = TestProvider::start().await;
    for prompt_count in [0_usize, 3, 4] {
        let cache = anthropic_options(json!({"cacheControl":{"type":"ephemeral"}}));
        let content = (0..prompt_count)
            .map(|index| {
                let mut text = TextPart::new(format!("Context {index}"));
                text.provider_options = Some(cache.clone());
                UserPromptPart::Text(text)
            })
            .collect();
        let mut options = CallOptions::new(vec![PromptMessage::user(content)]);
        options.tools = (0..2)
            .map(|index| {
                let mut tool = ToolDefinition::function(
                    format!("tool_{index}"),
                    None,
                    json!({"type":"object"}),
                );
                if let ToolDefinition::Function {
                    provider_options, ..
                } = &mut tool
                {
                    *provider_options = Some(cache.clone());
                }
                tool
            })
            .collect();
        let prepared = prepare(&test, "claude-sonnet-4-5", &options);
        let kept = json!(prepared.body)
            .to_string()
            .matches("cache_control")
            .count();
        assert_eq!(kept, (prompt_count + 2).min(4));
        let warnings = features(&prepared.warnings);
        assert_eq!(
            warnings,
            vec!["cacheControl breakpoint limit"; (prompt_count + 2).saturating_sub(4)]
        );
    }
}
