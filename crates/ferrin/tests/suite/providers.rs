//! Feature-gated provider re-exports.

#[cfg(feature = "openai")]
#[test]
fn openai_is_available_at_the_root_and_under_providers() {
    use ferrin::spec::LanguageModel as _;
    let provider =
        ferrin::openai::create_openai(ferrin::providers::openai::OpenAiSettings::default())
            .unwrap();
    assert_eq!(provider.responses("gpt-5").model_id().as_str(), "gpt-5");
}

#[cfg(feature = "anthropic")]
#[test]
fn anthropic_is_available() {
    use ferrin::spec::LanguageModel as _;
    let provider =
        ferrin::anthropic::create_anthropic(ferrin::anthropic::AnthropicSettings::default())
            .unwrap();
    assert_eq!(
        provider.messages("claude-sonnet-4-5").model_id().as_str(),
        "claude-sonnet-4-5"
    );
}

#[cfg(feature = "google")]
#[test]
fn google_is_available() {
    use ferrin::spec::LanguageModel as _;
    let provider =
        ferrin::google::create_google(ferrin::google::GoogleSettings::default()).unwrap();
    assert_eq!(
        provider
            .language_model("gemini-2.5-pro")
            .model_id()
            .as_str(),
        "gemini-2.5-pro"
    );
}

#[cfg(feature = "openai-compatible")]
#[test]
fn openai_compatible_is_available() {
    use ferrin::spec::LanguageModel as _;
    let settings = ferrin::openai_compatible::OpenAiCompatibleSettings::new(
        "acme",
        url::Url::parse("https://api.acme.test/v1").unwrap(),
    );
    let provider = ferrin::openai_compatible::create_openai_compatible(settings).unwrap();
    assert_eq!(
        provider.chat("acme-large").model_id().as_str(),
        "acme-large"
    );
    assert_eq!(provider.chat("acme-large").provider().as_str(), "acme.chat");
}

#[cfg(feature = "mcp")]
#[test]
fn mcp_is_available() {
    let error = ferrin::mcp::McpError::invalid_argument("bad");
    assert!(matches!(
        error,
        ferrin::mcp::McpError::InvalidArgument { .. }
    ));
}

#[cfg(feature = "otel")]
#[test]
fn otel_is_available() {
    assert_eq!(ferrin::otel::semconv::OPERATION_CHAT, "chat");
}
