//! Pure helpers: capabilities, schema sanitizing, usage and base URLs.

use ferrin_anthropic::api_types::AnthropicUsage;
use ferrin_anthropic::api_types::UsageIteration;
use ferrin_anthropic::capabilities::model_capabilities;
use ferrin_anthropic::config::normalize_base_url;
use ferrin_anthropic::json_schema::sanitize_json_schema;
use ferrin_anthropic::usage::convert_usage;
use ferrin_anthropic::usage::map_stop_reason;
use ferrin_spec::FinishReasonKind;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn capabilities_follow_the_model_family() {
    let sonnet = model_capabilities("claude-sonnet-4-5-20250929");
    assert_eq!(sonnet.max_output_tokens, 64_000);
    assert!(sonnet.supports_structured_output);
    assert!(!sonnet.supports_adaptive_thinking);
    assert!(sonnet.is_known_model);

    let sonnet_4 = model_capabilities("claude-sonnet-4-20250514");
    assert_eq!(sonnet_4.max_output_tokens, 64_000);
    assert!(!sonnet_4.supports_structured_output);

    let opus = model_capabilities("claude-opus-4-6");
    assert!(opus.supports_adaptive_thinking);
    assert!(!opus.supports_xhigh_effort);

    let opus_5 = model_capabilities("claude-opus-5");
    assert!(opus_5.rejects_sampling_parameters);
    assert!(opus_5.rejects_thinking_disabled_above_high_effort);

    let haiku_3 = model_capabilities("claude-3-haiku-20240307");
    assert_eq!(haiku_3.max_output_tokens, 4_096);
    assert!(haiku_3.is_known_model);

    let legacy = model_capabilities("claude-2.1");
    assert_eq!(legacy.max_output_tokens, 4_096);
    assert!(!legacy.is_known_model);

    let future = model_capabilities("claude-zeta-9");
    assert_eq!(future.max_output_tokens, 128_000);
    assert!(future.supports_adaptive_thinking);
    assert!(!future.is_known_model);

    let other = model_capabilities("my-fine-tune");
    assert_eq!(other.max_output_tokens, 4_096);
    assert!(!other.supports_structured_output);
}

#[test]
fn json_schema_sanitizing_closes_objects_and_moves_constraints() {
    let schema = json!({
        "type": "object",
        "properties": {
            "email": {"type": "string", "format": "phone", "minLength": 3},
            "count": {"type": "integer", "minimum": 0, "description": "How many"},
            "tags": {"type": "array", "items": {"type": "string"}, "maxItems": 5},
            "nested": {"type": "object", "properties": {"ok": {"type": "boolean", "default": false}}}
        },
        "required": ["email"]
    });
    let sanitized = sanitize_json_schema(&schema);
    assert_eq!(sanitized["additionalProperties"], json!(false));
    assert_eq!(sanitized["required"], json!(["email"]));
    assert!(sanitized["properties"]["email"].get("minLength").is_none());
    assert!(sanitized["properties"]["email"].get("format").is_none());
    assert_eq!(
        sanitized["properties"]["email"]["description"],
        json!("min length: 3; format: phone.")
    );
    assert_eq!(
        sanitized["properties"]["count"]["description"],
        json!("How many\nminimum: 0.")
    );
    assert_eq!(
        sanitized["properties"]["tags"]["description"],
        json!("max items: 5.")
    );
    assert_eq!(
        sanitized["properties"]["nested"]["additionalProperties"],
        json!(false)
    );
    assert!(sanitized["properties"]["nested"].get("required").is_none());
    assert_eq!(
        sanitized["properties"]["nested"]["properties"]["ok"]["default"],
        json!(false)
    );
}

#[test]
fn usage_sums_cache_tokens_and_executor_iterations() {
    let usage = AnthropicUsage {
        input_tokens: Some(10),
        output_tokens: Some(20),
        cache_creation_input_tokens: Some(5),
        cache_read_input_tokens: Some(15),
        ..AnthropicUsage::default()
    };
    let converted = convert_usage(
        &usage,
        Some(json!({"input_tokens": 10}).as_object().cloned().unwrap()),
    );
    assert_eq!(converted.input.total, Some(30));
    assert_eq!(converted.input.no_cache, Some(10));
    assert_eq!(converted.input.cache_read, Some(15));
    assert_eq!(converted.input.cache_write, Some(5));
    assert_eq!(converted.output.total, Some(20));
    assert_eq!(converted.output.reasoning, None);
    assert!(converted.raw.is_some());

    let iteration = |kind: &str, input: u64, output: u64| UsageIteration {
        kind: kind.to_owned(),
        input_tokens: Some(input),
        output_tokens: Some(output),
        ..UsageIteration::default()
    };
    let compacted = AnthropicUsage {
        input_tokens: Some(100),
        output_tokens: Some(50),
        iterations: Some(vec![
            iteration("compaction", 900, 40),
            iteration("message", 100, 50),
            iteration("advisor_message", 30, 10),
        ]),
        ..AnthropicUsage::default()
    };
    let converted = convert_usage(&compacted, None);
    assert_eq!(converted.input.total, Some(1000));
    assert_eq!(converted.output.total, Some(90));

    let fallback = AnthropicUsage {
        input_tokens: Some(100),
        output_tokens: Some(50),
        iterations: Some(vec![
            iteration("message", 100, 0),
            iteration("fallback_message", 100, 50),
        ]),
        ..AnthropicUsage::default()
    };
    let converted = convert_usage(&fallback, None);
    assert_eq!(converted.input.total, Some(100));
    assert_eq!(converted.output.total, Some(50));
}

#[test]
fn stop_reasons_map_to_unified_kinds() {
    assert_eq!(
        map_stop_reason(Some("end_turn"), false).unified,
        FinishReasonKind::Stop
    );
    assert_eq!(
        map_stop_reason(Some("pause_turn"), false).unified,
        FinishReasonKind::Stop
    );
    assert_eq!(
        map_stop_reason(Some("stop_sequence"), false).unified,
        FinishReasonKind::Stop
    );
    assert_eq!(
        map_stop_reason(Some("tool_use"), false).unified,
        FinishReasonKind::ToolCalls
    );
    assert_eq!(
        map_stop_reason(Some("tool_use"), true).unified,
        FinishReasonKind::Stop
    );
    assert_eq!(
        map_stop_reason(Some("max_tokens"), false).unified,
        FinishReasonKind::Length
    );
    assert_eq!(
        map_stop_reason(Some("model_context_window_exceeded"), false).unified,
        FinishReasonKind::Length
    );
    assert_eq!(
        map_stop_reason(Some("refusal"), false).unified,
        FinishReasonKind::ContentFilter
    );
    assert_eq!(
        map_stop_reason(Some("weird"), false).unified,
        FinishReasonKind::Other
    );
    assert_eq!(map_stop_reason(None, false).raw, None);
}

#[test]
fn base_urls_are_normalized() {
    assert_eq!(
        normalize_base_url("https://api.anthropic.com")
            .unwrap()
            .as_str(),
        "https://api.anthropic.com/v1"
    );
    assert_eq!(
        normalize_base_url("https://api.anthropic.com/")
            .unwrap()
            .as_str(),
        "https://api.anthropic.com/v1"
    );
    assert_eq!(
        normalize_base_url("https://proxy.test/custom/")
            .unwrap()
            .as_str(),
        "https://proxy.test/custom"
    );
    assert!(normalize_base_url("not a url").is_err());
}

#[test]
fn schema_references_retain_definitions_and_scope() {
    for definitions in ["$defs", "definitions"] {
        let reference = format!("#/{definitions}/Node");
        let schema = json!({
            "$id": "https://example.test/schema.json",
            "$ref": reference,
            definitions: {"Node": {
                "type":"object",
                "properties": {
                    "next": {"anyOf": [{"$ref":reference}, {"type":"null"}]},
                    "name": {"$id":"name.json", "$ref":"#/$defs/Name", "$defs":{"Name":{"type":"string"}}}
                },
                "required":["name"]
            }}
        });
        let mut expected = schema.clone();
        expected[definitions]["Node"]["additionalProperties"] = json!(false);
        assert_eq!(sanitize_json_schema(&schema), expected);
    }
}
