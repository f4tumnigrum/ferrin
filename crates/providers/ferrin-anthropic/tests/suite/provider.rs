//! Provider construction, identities and request headers.

use ferrin_anthropic::AnthropicSettings;
use ferrin_anthropic::create_anthropic;
use ferrin_spec::CallOptions;
use ferrin_spec::Headers;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::ProviderError;
use ferrin_spec::provider::Provider;
use http::Method;
use pretty_assertions::assert_eq;
use secrecy::SecretString;
use serde_json::json;
use url::Url;

use super::common::TestProvider;

fn settings(base_url: &str) -> AnthropicSettings {
    AnthropicSettings {
        base_url: Some(Url::parse(base_url).unwrap()),
        api_key: Some(SecretString::from("test-key".to_owned())),
        ..AnthropicSettings::default()
    }
}

#[test]
fn provider_and_model_ids_follow_the_name_family_scheme() {
    let provider = create_anthropic(settings("https://example.test/v1")).unwrap();
    assert_eq!(provider.provider_id().as_str(), "anthropic");
    assert_eq!(
        provider.messages("claude-sonnet-4-5").provider().as_str(),
        "anthropic.messages"
    );
    assert_eq!(
        provider.chat("claude-sonnet-4-5").provider().as_str(),
        "anthropic.messages"
    );
    assert_eq!(
        ferrin_spec::Batch::provider(&provider.batch()).as_str(),
        "anthropic.batch"
    );
    assert_eq!(
        ferrin_spec::Files::provider(&provider.files()).as_str(),
        "anthropic.files"
    );
    assert_eq!(
        ferrin_spec::Skills::provider(&provider.skills()).as_str(),
        "anthropic.skills"
    );
    assert!(provider.language_model("claude-sonnet-4-5").is_ok());
    assert!(provider.embedding_model("voyage-3").is_err());
    assert!(provider.image_model("x").is_err());
    assert!(Provider::files(&provider).is_some());
    assert!(Provider::skills(&provider).is_some());
    assert!(Provider::batch(&provider).is_some());
    assert!(Provider::realtime(&provider).is_none());
}

#[test]
fn custom_name_prefixes_every_provider_id() {
    let mut settings = settings("https://example.test/v1");
    settings.name = Some("myclaude".to_owned());
    let provider = create_anthropic(settings).unwrap();
    assert_eq!(provider.provider_id().as_str(), "myclaude");
    assert_eq!(
        provider.messages("claude-sonnet-4-5").provider().as_str(),
        "myclaude.messages"
    );
}

#[test]
fn bare_origin_base_url_gets_the_v1_path() {
    let provider = create_anthropic(settings("https://example.test")).unwrap();
    assert_eq!(
        provider.config().base_url.as_str(),
        "https://example.test/v1"
    );
    let provider = create_anthropic(settings("https://example.test/v1/")).unwrap();
    assert_eq!(
        provider.config().base_url.as_str(),
        "https://example.test/v1"
    );
    let provider = create_anthropic(settings("https://proxy.test/anthropic")).unwrap();
    assert_eq!(
        provider.config().base_url.as_str(),
        "https://proxy.test/anthropic"
    );
}

#[test]
fn two_credentials_are_rejected() {
    let mut settings = settings("https://example.test/v1");
    settings.auth_token = Some(SecretString::from("token".to_owned()));
    let error = create_anthropic(settings).unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn requests_carry_credential_version_betas_and_user_agent() {
    let test = TestProvider::start_with(|mut settings| {
        settings
            .headers
            .insert("anthropic-beta", "Config-Beta, ")
            .unwrap();
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/messages", "messages", "text-basic");
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options
        .headers
        .insert("anthropic-beta", "call-beta")
        .unwrap();
    options.headers.insert("x-trace", "trace-1").unwrap();
    options.tools = vec![ToolDefinition::function(
        "lookup",
        None,
        json!({"type": "object", "properties": {}}),
    )];
    test.provider
        .messages("claude-sonnet-4-5")
        .do_generate(options)
        .await
        .unwrap();
    let request = test.only_request();
    assert_eq!(request.header("x-api-key"), Some("test-key"));
    assert_eq!(request.header("authorization"), None);
    assert_eq!(request.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(request.header("x-trace"), Some("trace-1"));
    assert_eq!(
        request.header("anthropic-beta"),
        Some("call-beta,config-beta,structured-outputs-2025-11-13")
    );
    let user_agent = request.header("user-agent").unwrap();
    assert!(user_agent.contains("ferrin-anthropic/"), "{user_agent}");
}

#[tokio::test]
async fn auth_token_is_sent_as_a_bearer_token() {
    let test = TestProvider::start_with(|mut settings| {
        settings.api_key = None;
        settings.auth_token = Some(SecretString::from("test-token".to_owned()));
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/messages", "messages", "text-basic");
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
    options.headers = Headers::new();
    test.provider
        .messages("claude-sonnet-4-5")
        .do_generate(options)
        .await
        .unwrap();
    let request = test.only_request();
    assert_eq!(request.header("authorization"), Some("Bearer test-token"));
    assert_eq!(request.header("x-api-key"), None);
    assert_eq!(request.header("anthropic-beta"), None);
}
