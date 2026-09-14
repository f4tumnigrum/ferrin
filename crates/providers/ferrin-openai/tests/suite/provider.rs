//! Provider construction, identities and request headers.

use ferrin_openai::OpenAiSettings;
use ferrin_openai::create_openai;
use ferrin_spec::CallOptions;
use ferrin_spec::Headers;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::provider::Provider;
use http::Method;
use pretty_assertions::assert_eq;
use secrecy::SecretString;
use url::Url;

use super::common::TestProvider;

#[test]
fn provider_and_model_ids_follow_the_name_family_scheme() {
    let provider = create_openai(OpenAiSettings {
        base_url: Some(Url::parse("https://example.test/v1").unwrap()),
        api_key: Some(SecretString::from("test-key".to_owned())),
        ..OpenAiSettings::default()
    })
    .unwrap();
    assert_eq!(provider.provider_id().as_str(), "openai");
    assert_eq!(
        provider.responses("gpt-5").provider().as_str(),
        "openai.responses"
    );
    assert_eq!(provider.chat("gpt-4o").provider().as_str(), "openai.chat");
    assert_eq!(
        provider
            .completion("gpt-3.5-turbo-instruct")
            .provider()
            .as_str(),
        "openai.completion"
    );
    assert_eq!(
        ferrin_spec::EmbeddingModel::provider(&provider.embedding("text-embedding-3-small"))
            .as_str(),
        "openai.embedding"
    );
    assert_eq!(
        ferrin_spec::ImageModel::provider(&provider.image("gpt-image-1")).as_str(),
        "openai.image"
    );
    assert_eq!(
        ferrin_spec::SpeechModel::provider(&provider.speech("gpt-4o-mini-tts")).as_str(),
        "openai.speech"
    );
    assert_eq!(
        ferrin_spec::TranscriptionModel::provider(&provider.transcription("whisper-1")).as_str(),
        "openai.transcription"
    );
    assert_eq!(
        ferrin_spec::Batch::provider(&provider.batch()).as_str(),
        "openai.batch"
    );
    assert!(provider.language_model("gpt-5").is_ok());
    assert!(Provider::realtime(&provider).is_some());
    assert!(Provider::files(&provider).is_some());
    assert!(Provider::skills(&provider).is_some());
    assert!(Provider::batch(&provider).is_some());
}

#[test]
fn custom_name_prefixes_every_provider_id() {
    let provider = create_openai(OpenAiSettings {
        base_url: Some(Url::parse("https://example.test/v1").unwrap()),
        api_key: Some(SecretString::from("test-key".to_owned())),
        name: Some("azure".to_owned()),
        ..OpenAiSettings::default()
    })
    .unwrap();
    assert_eq!(provider.provider_id().as_str(), "azure");
    assert_eq!(provider.chat("gpt-4o").provider().as_str(), "azure.chat");
}

#[cfg(not(feature = "realtime"))]
#[test]
fn speech_translation_requires_the_realtime_feature() {
    let provider = create_openai(OpenAiSettings {
        base_url: Some(Url::parse("https://example.test/v1").unwrap()),
        api_key: Some(SecretString::from("test-key".to_owned())),
        ..OpenAiSettings::default()
    })
    .unwrap();
    let error = provider
        .speech_translation_model("gpt-realtime-translate")
        .unwrap_err();
    assert!(error.to_string().contains("realtime"), "{error}");
}

#[cfg(feature = "realtime")]
#[test]
fn speech_translation_is_available_with_the_realtime_feature() {
    let provider = create_openai(OpenAiSettings {
        base_url: Some(Url::parse("https://example.test/v1").unwrap()),
        api_key: Some(SecretString::from("test-key".to_owned())),
        ..OpenAiSettings::default()
    })
    .unwrap();
    assert!(
        provider
            .speech_translation_model("gpt-realtime-translate")
            .is_ok()
    );
}

#[tokio::test]
async fn requests_carry_auth_org_project_custom_and_user_agent_headers() {
    let test = TestProvider::start_with(|mut settings| {
        settings.organization = Some("org-123".to_owned());
        settings.project = Some("proj-456".to_owned());
        settings.headers = Headers::new().with("x-custom", "yes");
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/responses", "responses", "text-basic");
    let mut options = CallOptions::new(vec![PromptMessage::user_text("hi")]);
    options.headers = Headers::new().with("x-call", "call-header");
    test.provider
        .responses("gpt-5")
        .do_generate(options)
        .await
        .unwrap();
    let request = test.only_request();
    assert_eq!(request.header("authorization"), Some("Bearer test-key"));
    assert_eq!(request.header("openai-organization"), Some("org-123"));
    assert_eq!(request.header("openai-project"), Some("proj-456"));
    assert_eq!(request.header("x-custom"), Some("yes"));
    assert_eq!(request.header("x-call"), Some("call-header"));
    let user_agent = request.header("user-agent").unwrap();
    assert!(
        user_agent.contains(&format!("ferrin-openai/{}", ferrin_openai::VERSION)),
        "{user_agent}"
    );
}

#[tokio::test]
async fn missing_api_key_is_reported_by_the_first_request() {
    let test = TestProvider::start_with(|mut settings| {
        settings.api_key = None;
        settings
    })
    .await;
    // The environment variable may be set on a developer machine; only assert
    // the error shape when it is absent.
    if std::env::var_os("OPENAI_API_KEY").is_some() {
        return;
    }
    let error = test
        .provider
        .responses("gpt-5")
        .do_generate(CallOptions::new(vec![PromptMessage::user_text("hi")]))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::LoadApiKey(_)), "{error:?}");
    assert_eq!(test.server.received_count(), 0);
}
