//! Request body assembly of `generateContent`: snapshots and warnings.

use ferrin_google::request::PreparedRequest;
use ferrin_spec::CallOptions;
use ferrin_spec::PromptMessage;
use ferrin_spec::ReasoningEffort;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::features;
use super::common::google_options;
use super::common::options_under;

fn prepare(test: &TestProvider, model: &str, options: &CallOptions) -> PreparedRequest {
    test.provider
        .language_model(model)
        .prepare_request(options)
        .unwrap()
}

fn weather_tool() -> ToolDefinition {
    ToolDefinition::function(
        "get_weather",
        Some("Weather lookup".to_owned()),
        json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
            "additionalProperties": false
        }),
    )
}

#[tokio::test]
async fn basic_settings_map_to_generation_config() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![
        PromptMessage::system("You are terse."),
        PromptMessage::user_text("Hello"),
        PromptMessage::assistant_text("Hi!"),
        PromptMessage::user_text("How are you?"),
    ]);
    options.max_output_tokens = Some(100);
    options.temperature = Some(0.3);
    options.top_k = Some(5);
    options.top_p = Some(0.9);
    options.seed = Some(7);
    options.stop_sequences = Some(vec!["END".to_owned()]);
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    insta::assert_json_snapshot!("request_basic", prepared.body);
}

#[tokio::test]
async fn penalties_warn_on_gemini_2_5_and_pass_through_elsewhere() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.presence_penalty = Some(0.1);
    options.frequency_penalty = Some(0.2);
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert_eq!(
        features(&prepared.warnings),
        vec!["frequencyPenalty", "presencePenalty"]
    );
    assert!(
        prepared.body["generationConfig"]
            .get("presencePenalty")
            .is_none()
    );
    let prepared = prepare(&test, "gemini-2.0-flash", &options);
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    assert_eq!(
        prepared.body["generationConfig"]["presencePenalty"],
        json!(0.1)
    );
    assert_eq!(
        prepared.body["generationConfig"]["frequencyPenalty"],
        json!(0.2)
    );
}

#[tokio::test]
async fn json_response_format_sets_mime_type_and_openapi_schema() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.response_format = Some(ResponseFormat::Json {
        schema: Some(json!({
            "type": "object",
            "properties": {"answer": {"type": ["string", "null"]}},
            "required": ["answer"]
        })),
        name: Some("answer".to_owned()),
        description: None,
    });
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert_eq!(
        prepared.body["generationConfig"]["responseMimeType"],
        json!("application/json")
    );
    insta::assert_json_snapshot!(
        "request_response_schema",
        prepared.body["generationConfig"]["responseSchema"]
    );
    options.provider_options = google_options(json!({"structuredOutputs": false}));
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert_eq!(
        prepared.body["generationConfig"]["responseMimeType"],
        json!("application/json")
    );
    assert!(
        prepared.body["generationConfig"]
            .get("responseSchema")
            .is_none()
    );
}

#[tokio::test]
async fn reasoning_effort_maps_to_thinking_budget_or_level() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.reasoning = ReasoningEffort::Low;
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    insta::assert_json_snapshot!(
        "request_thinking_gemini_2_5",
        prepared.body["generationConfig"]["thinkingConfig"]
    );
    let prepared = prepare(&test, "gemini-3-pro-preview", &options);
    assert_eq!(
        prepared.body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        json!("low")
    );
    options.reasoning = ReasoningEffort::None;
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert_eq!(
        prepared.body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        json!(0)
    );
    options.reasoning = ReasoningEffort::ProviderDefault;
    options.provider_options = google_options(json!({
        "thinkingConfig": {"thinkingBudget": 1024, "includeThoughts": true}
    }));
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert_eq!(
        prepared.body["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget": 1024, "includeThoughts": true})
    );
}

#[tokio::test]
async fn provider_options_map_to_the_wire_format() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.provider_options = google_options(json!({
        "cachedContent": "cachedContents/abc",
        "labels": {"team": "ferrin"},
        "serviceTier": "priority",
        "responseModalities": ["TEXT", "IMAGE"],
        "audioTimestamp": true,
        "mediaResolution": "MEDIA_RESOLUTION_LOW",
        "threshold": "BLOCK_ONLY_HIGH",
        "imageConfig": {
            "aspectRatio": "16:9",
            "imageSize": "2K",
            "personGeneration": "allow_adult",
            "imageOutputOptions": {"mimeType": "image/png"}
        },
        "streamFunctionCallArguments": true
    }));
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    insta::assert_json_snapshot!("request_provider_options", prepared.body);
    insta::assert_json_snapshot!("request_provider_options_warnings", prepared.warnings);
}

#[tokio::test]
async fn explicit_safety_settings_take_precedence_over_threshold() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.provider_options = google_options(json!({
        "threshold": "BLOCK_ONLY_HIGH",
        "safetySettings": [
            {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE"}
        ]
    }));
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert_eq!(
        prepared.body["safetySettings"],
        json!([{"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE"}])
    );
}

#[tokio::test]
async fn tools_and_tool_choice_map_to_function_declarations() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.tools = vec![
        weather_tool(),
        ToolDefinition::function(
            "tree",
            None,
            json!({
                "type": "object",
                "properties": {"child": {"$ref": "#/$defs/node"}},
                "$defs": {"node": {"type": "object", "properties": {"child": {"$ref": "#/$defs/node"}}}}
            }),
        ),
    ];
    options.tool_choice = Some(ToolChoice::Tool {
        tool_name: "get_weather".into(),
    });
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    insta::assert_json_snapshot!("request_tools", prepared.body["tools"]);
    assert_eq!(
        prepared.body["toolConfig"],
        json!({"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["get_weather"]}})
    );
    options.tool_choice = Some(ToolChoice::None);
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert_eq!(
        prepared.body["toolConfig"],
        json!({"functionCallingConfig": {"mode": "NONE"}})
    );
}

#[tokio::test]
async fn custom_name_options_override_the_canonical_key() {
    let test = TestProvider::start_with(|mut settings| {
        settings.name = Some("mygemini".to_owned());
        settings
    })
    .await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.provider_options =
        google_options(json!({"serviceTier": "standard", "cachedContent": "cachedContents/a"}));
    options.provider_options.extend(options_under(
        "mygemini",
        json!({"serviceTier": "priority"}),
    ));
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert_eq!(prepared.body["serviceTier"], json!("priority"));
    assert_eq!(prepared.body["cachedContent"], json!("cachedContents/a"));
}

#[tokio::test]
async fn unknown_option_keys_are_ignored() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    let expected = test
        .provider
        .language_model("gemini-2.5-flash")
        .prepare_request(&options)
        .unwrap();
    options.provider_options = google_options(json!({"unknownSetting": 1}));
    let actual = test
        .provider
        .language_model("gemini-2.5-flash")
        .prepare_request(&options)
        .unwrap();
    assert_eq!(
        (actual.body, actual.warnings),
        (expected.body, expected.warnings)
    );
}

#[tokio::test]
async fn explicit_thinking_fields_override_generic_reasoning() {
    let test = TestProvider::start().await;
    for (model, thinking) in [
        (
            "gemini-2.5-flash",
            json!({"thinkingBudget": 1024, "includeThoughts": false}),
        ),
        (
            "gemini-3-flash-preview",
            json!({"thinkingLevel": "low", "includeThoughts": false}),
        ),
    ] {
        let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
        options.reasoning = ReasoningEffort::High;
        options.provider_options = google_options(json!({"thinkingConfig": thinking}));
        let prepared = prepare(&test, model, &options);
        assert_eq!(
            prepared.body["generationConfig"]["thinkingConfig"],
            thinking
        );
    }
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.reasoning = ReasoningEffort::High;
    options.provider_options =
        google_options(json!({"thinkingConfig": {"includeThoughts": false}}));
    let prepared = prepare(&test, "gemini-2.5-flash", &options);
    assert_eq!(
        prepared.body["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget": 24576, "includeThoughts": false})
    );
}
