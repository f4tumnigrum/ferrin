//! Chat Completions: generate, request assembly, hooks.

use std::sync::Arc;

use ferrin_openai_compatible::ErrorStructure;
use ferrin_openai_compatible::MetadataExtractor;
use ferrin_openai_compatible::StreamMetadataExtractor;
use ferrin_openai_compatible::chat::output::convert_content;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::ReasoningEffort;
use ferrin_spec::language_model::ResponseFormat;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::example_options;
use super::common::options_under;

fn prompt() -> Vec<PromptMessage> {
    vec![
        PromptMessage::system("You are terse."),
        PromptMessage::user_text("Say hello"),
    ]
}

#[tokio::test]
async fn generate_maps_text_usage_and_metadata() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "text-basic");
    let result = test
        .provider
        .chat("example-model")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap();
    assert_eq!(result.content.len(), 1);
    assert_eq!(result.content[0].as_text(), Some("Hello from chat."));
    assert_eq!(result.finish_reason.unified, FinishReasonKind::Stop);
    assert_eq!(result.usage.input.total, Some(20));
    assert_eq!(result.usage.input.no_cache, Some(15));
    assert_eq!(result.usage.input.cache_read, Some(5));
    assert_eq!(result.usage.output.total, Some(10));
    assert_eq!(result.usage.output.text, Some(8));
    assert_eq!(result.usage.output.reasoning, Some(2));
    assert!(result.usage.raw.is_some());
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(
        JsonValue::Object(metadata["example"].clone()),
        json!({"acceptedPredictionTokens": 1, "rejectedPredictionTokens": 0})
    );
    assert_eq!(result.response.id.as_deref(), Some("chatcmpl_1"));
    assert_eq!(
        result
            .response
            .model_id
            .as_ref()
            .map(ferrin_spec::ModelId::as_str),
        Some("example-model-2026")
    );
    assert!(result.response.timestamp.is_some());
    assert!(result.warnings.is_empty());
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request,
        json!({
            "model": "example-model",
            "messages": [
                {"role": "system", "content": "You are terse."},
                {"role": "user", "content": "Say hello"}
            ]
        })
    );
}

#[tokio::test]
async fn generate_maps_reasoning_content() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "reasoning");
    let result = test
        .provider
        .chat("example-reasoner")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap();
    assert_eq!(
        result.content,
        vec![
            Content::text("Hello back."),
            Content::reasoning("The user greets me."),
        ]
    );
    assert_eq!(result.usage.output.reasoning, Some(4));
    assert_eq!(result.usage.output.text, Some(5));
}

#[tokio::test]
async fn generate_maps_tool_calls_and_thought_signatures() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "tool-call");
    let mut options = CallOptions::new(prompt());
    options.tools = vec![ToolDefinition::function(
        "get_weather",
        Some("Weather".to_owned()),
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    )];
    options.tool_choice = Some(ToolChoice::Required);
    let result = test
        .provider
        .chat("example-model")
        .do_generate(options)
        .await
        .unwrap();
    let first = result.content[0].as_tool_call().unwrap();
    assert_eq!(first.tool_call_id.as_str(), "call_c1");
    assert_eq!(first.tool_name.as_str(), "get_weather");
    assert_eq!(first.input, "{\"city\":\"Paris\"}");
    assert_eq!(first.provider_metadata, None);
    let second = result.content[1].as_tool_call().unwrap();
    assert_eq!(second.tool_call_id.as_str(), "id-0");
    assert_eq!(
        second.provider_metadata.as_ref().unwrap()["example"]["thoughtSignature"],
        json!("sig-123")
    );
    assert_eq!(result.finish_reason.unified, FinishReasonKind::ToolCalls);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request["tools"],
        json!([{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Weather",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
            }
        }])
    );
    assert_eq!(request["tool_choice"], json!("required"));
}

#[tokio::test]
async fn unauthorized_response_is_an_api_call_error() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "error-401");
    let error = test
        .provider
        .chat("example-model")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap_err();
    assert_eq!(error.status_code(), Some(StatusCode::UNAUTHORIZED));
    assert!(!error.is_retryable());
    assert!(
        error.to_string().contains("Incorrect API key provided"),
        "{error}"
    );
}

#[derive(Debug)]
struct DetailErrorStructure;

impl ErrorStructure for DetailErrorStructure {
    fn message(&self, body: &JsonValue) -> Option<String> {
        body.get("detail")?.as_str().map(str::to_owned)
    }

    fn is_retryable(&self, _status: StatusCode, body: Option<&JsonValue>) -> Option<bool> {
        body?.get("retry")?.as_bool()
    }
}

#[tokio::test]
async fn custom_error_structure_controls_message_and_retryability() {
    let test = TestProvider::start_with(|mut settings| {
        settings.error_structure = Some(Arc::new(DetailErrorStructure));
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "error-custom");
    let error = test
        .provider
        .chat("example-model")
        .do_generate(CallOptions::new(prompt()))
        .await
        .unwrap_err();
    assert_eq!(error.status_code(), Some(StatusCode::SERVICE_UNAVAILABLE));
    assert!(error.is_retryable());
    assert!(
        error
            .to_string()
            .contains("Model is not available in this region"),
        "{error}"
    );
}

#[tokio::test]
async fn request_snapshot_with_options_tools_and_structured_outputs() {
    let test = TestProvider::start_with(|mut settings| {
        settings.supports_structured_outputs = true;
        settings
    })
    .await;
    let mut options = CallOptions::new(prompt());
    options.max_output_tokens = Some(100);
    options.temperature = Some(0.5);
    options.top_p = Some(0.9);
    options.frequency_penalty = Some(0.1);
    options.presence_penalty = Some(0.2);
    options.stop_sequences = Some(vec!["END".to_owned()]);
    options.seed = Some(42);
    options.response_format = Some(ResponseFormat::Json {
        schema: Some(json!({"type": "object", "properties": {"a": {"type": "string"}}})),
        name: Some("answer".to_owned()),
        description: Some("The answer".to_owned()),
    });
    let mut tool = ToolDefinition::function(
        "get_weather",
        None,
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    );
    if let ToolDefinition::Function { strict, .. } = &mut tool {
        *strict = Some(true);
    }
    options.tools = vec![
        tool,
        ToolDefinition::provider("example.search", "search", JsonObject::new()),
    ];
    options.tool_choice = Some(ToolChoice::Tool {
        tool_name: "get_weather".into(),
    });
    options.provider_options = options_under("openaiCompatible", json!({"user": "shared"}));
    options.provider_options.extend(example_options(json!({
        "user": "u1",
        "reasoningEffort": "low",
        "textVerbosity": "low",
        "strictJsonSchema": false,
        "customFlag": true,
        "nested": {"a": 1}
    })));
    let prepared = test
        .provider
        .chat("example-model")
        .prepare_request(&options)
        .unwrap();
    assert_eq!(
        prepared.warnings,
        vec![Warning::unsupported("provider-defined tool example.search")]
    );
    assert_eq!(prepared.metadata_key, "example");
    insta::assert_json_snapshot!("chat_request_full", prepared.body);
}

#[tokio::test]
async fn json_schema_without_structured_outputs_falls_back_to_json_object() {
    let test = TestProvider::start().await;
    let mut options = CallOptions::new(prompt());
    options.response_format = Some(ResponseFormat::Json {
        schema: Some(json!({"type": "object"})),
        name: None,
        description: None,
    });
    options.top_k = Some(3);
    options.reasoning = ReasoningEffort::High;
    let prepared = test
        .provider
        .chat("example-model")
        .prepare_request(&options)
        .unwrap();
    let body = JsonValue::Object(prepared.body);
    assert_eq!(body["response_format"], json!({"type": "json_object"}));
    assert_eq!(body["reasoning_effort"], json!("high"));
    assert_eq!(
        prepared.warnings,
        vec![
            Warning::unsupported("topK"),
            Warning::unsupported_with_details(
                "responseFormat",
                "JSON response format schema is only supported with structuredOutputs"
            ),
        ]
    );
}

#[tokio::test]
async fn camel_case_and_deprecated_option_keys() {
    let test = TestProvider::start_with(|mut settings| {
        settings.name = "my-provider".to_owned();
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "text-basic");
    let model = test.provider.chat("example-model");
    assert_eq!(model.provider().as_str(), "my-provider.chat");

    let mut options = CallOptions::new(prompt());
    options.provider_options = options_under("myProvider", json!({"user": "camel"}));
    let prepared = model.prepare_request(&options).unwrap();
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    assert_eq!(prepared.metadata_key, "myProvider");
    assert_eq!(prepared.body["user"], json!("camel"));

    let mut options = CallOptions::new(prompt());
    options.provider_options = options_under("my-provider", json!({"user": "raw"}));
    options
        .provider_options
        .extend(options_under("openai-compatible", json!({"user": "old"})));
    let prepared = model.prepare_request(&options).unwrap();
    assert_eq!(prepared.metadata_key, "my-provider");
    assert_eq!(prepared.body["user"], json!("raw"));
    assert_eq!(
        prepared.warnings,
        vec![
            Warning::deprecated(
                "providerOptions key 'openai-compatible'",
                "Use 'openaiCompatible' instead."
            ),
            Warning::deprecated(
                "providerOptions key 'my-provider'",
                "Use 'myProvider' instead."
            ),
        ]
    );

    let result = model.do_generate(options).await.unwrap();
    assert!(
        result
            .provider_metadata
            .unwrap()
            .contains_key("my-provider")
    );
}

#[tokio::test]
async fn transform_request_body_and_convert_usage_hooks_are_applied() {
    let test = TestProvider::start_with(|mut settings| {
        settings.transform_request_body = Some(Arc::new(|mut body: JsonObject| {
            body.insert("extra".to_owned(), JsonValue::Bool(true));
            body.remove("temperature");
            body
        }));
        settings.convert_usage = Some(Arc::new(|usage| {
            Usage::totals(
                usage.prompt_tokens.unwrap_or(0) * 2,
                usage.completion_tokens.unwrap_or(0) * 2,
            )
        }));
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "text-basic");
    let mut options = CallOptions::new(prompt());
    options.temperature = Some(0.1);
    let result = test
        .provider
        .chat("example-model")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.usage, Usage::totals(40, 20));
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["extra"], json!(true));
    assert!(request.get("temperature").is_none());
}

#[derive(Debug)]
struct IdExtractor;

#[derive(Debug, Default)]
struct IdStreamExtractor {
    chunks: u64,
    id: Option<String>,
}

impl MetadataExtractor for IdExtractor {
    fn extract_metadata(&self, body: &JsonValue) -> Option<ProviderMetadata> {
        let id = body.get("id")?.as_str()?;
        let mut object = JsonObject::new();
        object.insert("responseId".to_owned(), JsonValue::from(id));
        let mut metadata = ProviderMetadata::new();
        metadata.insert("custom".to_owned(), object);
        Some(metadata)
    }

    fn stream_extractor(&self) -> Box<dyn StreamMetadataExtractor> {
        Box::new(IdStreamExtractor::default())
    }
}

impl StreamMetadataExtractor for IdStreamExtractor {
    fn process_chunk(&mut self, chunk: &JsonValue) {
        self.chunks += 1;
        if self.id.is_none() {
            self.id = chunk
                .get("id")
                .and_then(JsonValue::as_str)
                .map(str::to_owned);
        }
    }

    fn build_metadata(&mut self) -> Option<ProviderMetadata> {
        let mut object = JsonObject::new();
        object.insert("chunks".to_owned(), JsonValue::from(self.chunks));
        object.insert("responseId".to_owned(), json!(self.id));
        let mut metadata = ProviderMetadata::new();
        metadata.insert("custom".to_owned(), object);
        Some(metadata)
    }
}

#[tokio::test]
async fn metadata_extractor_adds_provider_metadata_to_generate_and_stream() {
    let test = TestProvider::start_with(|mut settings| {
        settings.metadata_extractor = Some(Arc::new(IdExtractor));
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "text-basic");
    let model = test.provider.chat("example-model");
    let result = model.do_generate(CallOptions::new(prompt())).await.unwrap();
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["custom"]["responseId"], json!("chatcmpl_1"));
    assert_eq!(metadata["example"]["acceptedPredictionTokens"], json!(1));

    let test = TestProvider::start_with(|mut settings| {
        settings.metadata_extractor = Some(Arc::new(IdExtractor));
        settings
    })
    .await;
    test.mount(
        Method::POST,
        "/v1/chat/completions",
        "chat",
        "text-basic-stream",
    );
    let stream = test
        .provider
        .chat("example-model")
        .do_stream(CallOptions::new(prompt()))
        .await
        .unwrap();
    let parts = collect_checked(stream).await;
    let Some(StreamPart::Finish {
        provider_metadata, ..
    }) = parts.last()
    else {
        panic!("expected finish, got {parts:#?}");
    };
    let metadata = provider_metadata.as_ref().unwrap();
    assert_eq!(metadata["custom"]["chunks"], json!(5));
    assert_eq!(metadata["custom"]["responseId"], json!("chatcmpl_s"));
    assert_eq!(metadata["example"]["acceptedPredictionTokens"], json!(1));
}

#[test]
fn content_arrays_yield_text_and_reasoning_parts() {
    let content: ferrin_openai_compatible::chat::api_types::ChatContent =
        serde_json::from_value(json!([
            {"type": "thinking", "thinking": [{"type": "text", "text": "Th"}, {"type": "text", "text": "ink"}]},
            {"type": "text", "text": "Answer"},
            {"type": "text", "text": ""},
            {"type": "image", "url": "ignored"}
        ]))
        .unwrap();
    assert_eq!(
        convert_content(Some(&content)),
        vec![Content::reasoning("Think"), Content::text("Answer")]
    );
    assert!(convert_content(None).is_empty());
}
