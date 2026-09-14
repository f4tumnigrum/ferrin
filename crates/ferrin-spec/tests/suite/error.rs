use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;
use static_assertions::const_assert;
use url::Url;

use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::ModelKind;
use ferrin_spec::error::NoSuchModelError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::TypeValidationContext;
use ferrin_spec::error::TypeValidationError;
use ferrin_spec::error::default_retryable;

const_assert!(size_of::<ProviderError>() <= 128);

fn url() -> Url {
    Url::parse("https://api.example.com/v1/chat").unwrap()
}

#[test]
fn default_retryable_matches_reference_statuses() {
    assert!(default_retryable(None));
    for status in [408, 409, 429, 500, 502, 503] {
        assert!(
            default_retryable(Some(StatusCode::from_u16(status).unwrap())),
            "{status}"
        );
    }
    for status in [400, 401, 403, 404, 422] {
        assert!(
            !default_retryable(Some(StatusCode::from_u16(status).unwrap())),
            "{status}"
        );
    }
}

#[test]
fn is_retryable_only_for_api_call() {
    let retryable: ProviderError = ApiCallError::new("rate limited", url())
        .with_status(StatusCode::TOO_MANY_REQUESTS)
        .into();
    assert!(retryable.is_retryable());
    assert_eq!(retryable.status_code(), Some(StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(retryable.kind_name(), "api_call");

    let overridden: ProviderError = ApiCallError::new("bad", url())
        .with_status(StatusCode::BAD_REQUEST)
        .retryable(true)
        .into();
    assert!(overridden.is_retryable());

    let unsupported = ProviderError::unsupported("video");
    assert!(!unsupported.is_retryable());
    assert_eq!(unsupported.status_code(), None);
    assert_eq!(
        unsupported.to_string(),
        "`video` functionality not supported"
    );

    let other = ProviderError::message("boom");
    assert_eq!(other.to_string(), "boom");
    assert_eq!(other.kind_name(), "other");
}

#[test]
fn api_call_display_is_truncated() {
    let long = "x".repeat(5000);
    let error = ApiCallError::new(long, url());
    let shown = error.to_string();
    assert!(shown.len() < 2100);
    assert!(shown.ends_with("... [truncated]"));

    let multibyte = "é".repeat(1500);
    let shown = ApiCallError::new(multibyte, url()).to_string();
    assert!(shown.ends_with("... [truncated]"));
    assert!(shown.len() <= 2048 + "... [truncated]".len());
}

#[test]
fn no_such_model_messages() {
    let error = NoSuchModelError::new("gpt-x", ModelKind::Language);
    assert_eq!(error.to_string(), "no such language model: gpt-x");
    let error = NoSuchModelError::unsupported_kind(&"anthropic".into(), "m", ModelKind::Image);
    assert_eq!(
        error.to_string(),
        "provider anthropic does not expose image models (requested m)"
    );
    assert_eq!(
        serde_json::to_value(ModelKind::SpeechTranslation).unwrap(),
        json!("speech-translation")
    );
}

#[test]
fn type_validation_wrap_deduplicates() {
    let inner = TypeValidationError::new(json!({ "a": 1 }), std::fmt::Error).with_context(
        TypeValidationContext {
            field: Some("input".to_owned()),
            entity_name: Some("tool".to_owned()),
            entity_id: Some("call_1".to_owned()),
        },
    );
    let message = inner.to_string();
    assert_eq!(
        message,
        "type validation failed for input (tool, id: \"call_1\"): an error occurred when formatting an argument"
    );

    let context = inner.context.clone().map(|context| *context);
    let wrapped = TypeValidationError::wrap(json!({ "a": 1 }), Box::new(inner), context);
    assert!(
        wrapped
            .cause
            .downcast_ref::<TypeValidationError>()
            .is_none()
    );

    let nested = TypeValidationError::wrap(
        json!({ "b": 2 }),
        Box::new(TypeValidationError::new(json!(1), std::fmt::Error)),
        None,
    );
    assert!(nested.cause.downcast_ref::<TypeValidationError>().is_some());
}
