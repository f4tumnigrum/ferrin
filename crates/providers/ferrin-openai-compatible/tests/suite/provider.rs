//! Provider construction, identities, URLs and request headers.

use ferrin_openai_compatible::OpenAiCompatibleSettings;
use ferrin_openai_compatible::create_openai_compatible;
use ferrin_spec::CallOptions;
use ferrin_spec::Headers;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::provider::Provider;
use http::Method;
use pretty_assertions::assert_eq;
use url::Url;

use super::common::TestProvider;

fn settings(name: &str) -> OpenAiCompatibleSettings {
    OpenAiCompatibleSettings::new(name, Url::parse("https://example.test/v1").unwrap())
}

#[test]
fn provider_and_model_ids_follow_the_name_family_scheme() {
    let provider = create_openai_compatible(settings("example")).unwrap();
    assert_eq!(provider.provider_id().as_str(), "example");
    assert_eq!(provider.chat("m").provider().as_str(), "example.chat");
    assert_eq!(
        provider.completion("m").provider().as_str(),
        "example.completion"
    );
    assert_eq!(
        ferrin_spec::EmbeddingModel::provider(&provider.embedding("m")).as_str(),
        "example.embedding"
    );
    assert_eq!(
        ferrin_spec::ImageModel::provider(&provider.image("m")).as_str(),
        "example.image"
    );
    assert!(provider.language_model("m").is_ok());
    assert!(provider.embedding_model("m").is_ok());
    assert!(provider.image_model("m").is_ok());
    assert!(Provider::files(&provider).is_none());
    assert!(Provider::batch(&provider).is_none());
}

#[test]
fn empty_and_dotted_names_are_rejected() {
    for name in ["", "  ", "my.provider"] {
        let error = create_openai_compatible(settings(name)).unwrap_err();
        assert!(
            matches!(error, ProviderError::InvalidArgument(_)),
            "{name:?}: {error:?}"
        );
    }
}

#[tokio::test]
async fn base_url_trailing_slash_is_removed_and_query_params_are_appended() {
    let test = TestProvider::start_with(|mut settings| {
        settings.base_url = settings.base_url.join("v1/").unwrap();
        settings.query_params = vec![("api-version".to_owned(), "2026-01".to_owned())];
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "text-basic");
    test.provider
        .chat("m")
        .do_generate(CallOptions::new(vec![PromptMessage::user_text("hi")]))
        .await
        .unwrap();
    let request = test.only_request();
    assert_eq!(request.path, "/v1/chat/completions");
    assert_eq!(request.query.as_deref(), Some("api-version=2026-01"));
}

#[tokio::test]
async fn requests_carry_bearer_custom_and_user_agent_headers() {
    let test = TestProvider::start_with(|mut settings| {
        settings.headers = Headers::new().with("x-custom", "yes");
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "text-basic");
    let mut options = CallOptions::new(vec![PromptMessage::user_text("hi")]);
    options.headers = Headers::new().with("x-call", "1");
    test.provider.chat("m").do_generate(options).await.unwrap();
    let request = test.only_request();
    assert_eq!(request.header("authorization"), Some("Bearer test-key"));
    assert_eq!(request.header("x-custom"), Some("yes"));
    assert_eq!(request.header("x-call"), Some("1"));
    assert_eq!(request.header("content-type"), Some("application/json"));
    let user_agent = request.header("user-agent").unwrap();
    assert!(
        user_agent.contains("ferrin-openai-compatible/"),
        "{user_agent}"
    );
}

#[tokio::test]
async fn without_a_key_no_authorization_header_is_sent() {
    let test = TestProvider::start_with(|mut settings| {
        settings.api_key = None;
        settings.api_key_env = Some("FERRIN_TEST_UNSET_COMPAT_KEY".to_owned());
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/chat/completions", "chat", "text-basic");
    test.provider
        .chat("m")
        .do_generate(CallOptions::new(vec![PromptMessage::user_text("hi")]))
        .await
        .unwrap();
    assert_eq!(test.only_request().header("authorization"), None);
}
