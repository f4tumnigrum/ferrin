//! Provider construction, identities, URLs and request headers.

use ferrin_google::GoogleSettings;
use ferrin_google::config::GoogleConfig;
use ferrin_google::create_google;
use ferrin_spec::Batch;
use ferrin_spec::CallOptions;
use ferrin_spec::EmbeddingModel;
use ferrin_spec::Files;
use ferrin_spec::Headers;
use ferrin_spec::ImageModel;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::RealtimeFactory;
use ferrin_spec::error::ProviderError;
use ferrin_spec::provider::Provider;
use ferrin_spec::speech_model::SpeechModel;
use ferrin_spec::transcription_model::TranscriptionModel;
use ferrin_spec::video_model::VideoModel;
use http::Method;
use pretty_assertions::assert_eq;
use secrecy::SecretString;
use url::Url;

use super::common::TestProvider;

fn settings(base_url: &str) -> GoogleSettings {
    GoogleSettings {
        base_url: Some(Url::parse(base_url).unwrap()),
        api_key: Some(SecretString::from("test-key".to_owned())),
        ..GoogleSettings::default()
    }
}

#[test]
fn provider_and_model_ids_follow_the_name_family_scheme() {
    let provider = create_google(settings("https://example.test/v1beta")).unwrap();
    assert_eq!(provider.provider_id().as_str(), "google");
    assert_eq!(
        provider
            .language_model("gemini-2.5-flash")
            .provider()
            .as_str(),
        "google.generative-ai"
    );
    assert_eq!(
        provider.chat("gemini-2.5-flash").provider().as_str(),
        "google.generative-ai"
    );
    assert_eq!(
        provider.embedding("text-embedding-004").provider().as_str(),
        "google"
    );
    assert_eq!(
        provider.image("gemini-2.5-flash-image").provider().as_str(),
        "google"
    );
    assert_eq!(
        provider
            .speech("gemini-2.5-flash-preview-tts")
            .provider()
            .as_str(),
        "google.speech"
    );
    assert_eq!(
        provider
            .transcription("gemini-3.1-flash-preview")
            .provider()
            .as_str(),
        "google.transcription"
    );
    assert_eq!(
        provider
            .video("veo-3.1-generate-preview")
            .provider()
            .as_str(),
        "google"
    );
    assert_eq!(Files::provider(&provider.files()).as_str(), "google");
    assert_eq!(Batch::provider(&provider.batch()).as_str(), "google.batch");
    assert_eq!(
        RealtimeFactory::provider(&provider.realtime()).as_str(),
        "google.realtime"
    );
    assert!(Provider::language_model(&provider, "gemini-2.5-flash").is_ok());
    assert!(provider.embedding_model("text-embedding-004").is_ok());
    assert!(provider.image_model("gemini-2.5-flash-image").is_ok());
    assert!(
        provider
            .speech_model("gemini-2.5-flash-preview-tts")
            .is_ok()
    );
    assert!(
        provider
            .transcription_model("gemini-3.1-flash-preview")
            .is_ok()
    );
    assert!(provider.video_model("veo-3.1-generate-preview").is_ok());
    assert!(provider.reranking_model("x").is_err());
    #[cfg(feature = "realtime")]
    assert!(provider.speech_translation_model("x").is_ok());
    #[cfg(not(feature = "realtime"))]
    assert!(provider.speech_translation_model("x").is_err());
    assert!(Provider::files(&provider).is_some());
    assert!(Provider::batch(&provider).is_some());
    assert!(Provider::realtime(&provider).is_some());
    assert!(Provider::skills(&provider).is_none());
}

#[test]
fn custom_name_prefixes_every_provider_id() {
    let mut settings = settings("https://example.test/v1beta");
    settings.name = Some("mygemini".to_owned());
    let provider = create_google(settings).unwrap();
    assert_eq!(provider.provider_id().as_str(), "mygemini");
    assert_eq!(
        provider
            .language_model("gemini-2.5-flash")
            .provider()
            .as_str(),
        "mygemini.generative-ai"
    );
    assert_eq!(
        Batch::provider(&provider.batch()).as_str(),
        "mygemini.batch"
    );
    assert_eq!(provider.config().options_key(), "mygemini");
}

#[test]
fn base_url_loses_its_trailing_slash_and_urls_are_derived_from_it() {
    let provider = create_google(settings("https://example.test/v1beta/")).unwrap();
    let config = provider.config();
    assert_eq!(config.base_url.as_str(), "https://example.test/v1beta");
    assert_eq!(
        GoogleConfig::model_path("gemini-2.5-flash"),
        "models/gemini-2.5-flash"
    );
    assert_eq!(
        GoogleConfig::model_path("tunedModels/my-model"),
        "tunedModels/my-model"
    );
    assert_eq!(
        config
            .model_url("gemini-2.5-flash", "generateContent")
            .as_str(),
        "https://example.test/v1beta/models/gemini-2.5-flash:generateContent"
    );
    assert_eq!(
        config.url("batches/abc").as_str(),
        "https://example.test/v1beta/batches/abc"
    );
    assert_eq!(
        config.origin_url("/upload/v1beta/files").as_str(),
        "https://example.test/upload/v1beta/files"
    );
    assert_eq!(
        config.websocket_url("svc.Bidi").as_str(),
        "wss://example.test/ws/svc.Bidi"
    );
    let local = create_google(settings("http://127.0.0.1:8080/v1alpha")).unwrap();
    assert_eq!(
        local.config().websocket_url("svc.Bidi").as_str(),
        "ws://127.0.0.1:8080/ws/svc.Bidi"
    );
}

#[tokio::test]
async fn requests_carry_the_api_key_configured_and_call_headers() {
    let test = TestProvider::start_with(|mut settings| {
        settings.headers = Headers::new().with("x-config", "cfg");
        settings
    })
    .await;
    test.mount(
        Method::POST,
        "/v1beta/models/gemini-3-pro-preview:generateContent",
        "generate",
        "text",
    );
    let mut options = CallOptions::new(vec![PromptMessage::user_text("Hi")]);
    options.headers = Headers::new().with("x-call", "call");
    test.provider
        .language_model("gemini-3-pro-preview")
        .do_generate(options)
        .await
        .unwrap();
    let request = test.only_request();
    assert_eq!(request.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(request.header("x-config"), Some("cfg"));
    assert_eq!(request.header("x-call"), Some("call"));
    assert_eq!(request.header("content-type"), Some("application/json"));
    let user_agent = request.header("user-agent").unwrap();
    assert!(user_agent.contains("ferrin-google/"), "{user_agent}");
}

#[tokio::test]
async fn missing_api_key_fails_on_the_first_request() {
    let test = TestProvider::start_with(|mut settings| {
        settings.api_key = None;
        settings
    })
    .await;
    if ferrin_provider_util::settings::env_var("GOOGLE_GENERATIVE_AI_API_KEY").is_some() {
        // The environment supplies a key; the lazy fallback would succeed.
        return;
    }
    let error = test
        .provider
        .language_model("gemini-2.5-flash")
        .do_generate(CallOptions::new(vec![PromptMessage::user_text("Hi")]))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::LoadApiKey(_)), "{error:?}");
    assert_eq!(test.server.received_count(), 0);
}
