//! Error bodies and error frames.

use ferrin_openai_compatible::DefaultErrorStructure;
use ferrin_openai_compatible::ErrorStructure;
use ferrin_openai_compatible::error::early_error;
use ferrin_openai_compatible::error::parse_frame_error;
use ferrin_openai_compatible::error::stream_error_for_frame;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

#[test]
fn default_structure_reads_the_openai_error_body() {
    let structure = DefaultErrorStructure;
    assert_eq!(
        structure.message(&json!({"error": {"message": "bad", "type": "invalid_request_error"}})),
        Some("bad".to_owned())
    );
    assert_eq!(structure.message(&json!({"detail": "other"})), None);
    assert_eq!(
        structure.is_retryable(http::StatusCode::TOO_MANY_REQUESTS, None),
        None
    );
}

#[test]
fn frame_codes_are_mapped_heuristically() {
    let structure = DefaultErrorStructure;
    let cases = [
        (json!({"error": {"code": "429", "message": "x"}}), 429, true),
        (
            json!({"error": {"code": "insufficient_quota", "message": "x"}}),
            429,
            false,
        ),
        (
            json!({"error": {"type": "authentication_error", "message": "x"}}),
            401,
            false,
        ),
        (
            json!({"error": {"type": "permission_error", "message": "x"}}),
            403,
            false,
        ),
        (
            json!({"error": {"code": "model_not_found", "message": "x"}}),
            404,
            false,
        ),
        (
            json!({"error": {"type": "invalid_request_error", "message": "x"}}),
            400,
            false,
        ),
        (
            json!({"error": {"code": "server_overloaded", "message": "x"}}),
            503,
            true,
        ),
        (
            json!({"error": {"code": "timeout", "message": "x"}}),
            504,
            true,
        ),
        (
            json!({"error": {"code": "mystery", "message": "x"}}),
            500,
            true,
        ),
    ];
    for (frame, status, retryable) in cases {
        let error = parse_frame_error(&structure, &frame).unwrap();
        assert_eq!(error.status_code(), status, "{frame}");
        assert_eq!(error.is_retryable(), retryable, "{frame}");
    }
    assert!(parse_frame_error(&structure, &json!({"choices": []})).is_none());
}

#[test]
fn stream_and_early_errors_carry_status_and_fallback_messages() {
    let structure = DefaultErrorStructure;
    let frame = json!({"error": {"code": "rate_limit_exceeded", "message": "Rate limit reached"}});
    let stream_error = stream_error_for_frame(&structure, &frame);
    assert_eq!(stream_error.message, "Rate limit reached");
    assert_eq!(stream_error.status_code, Some(429));
    assert_eq!(stream_error.is_retryable, Some(true));
    let generic = stream_error_for_frame(&structure, &json!({"weird": true}));
    assert_eq!(generic.message, "stream error");

    let url = Url::parse("https://example.test/v1/chat/completions").unwrap();
    let api = early_error(&structure, url.clone(), &frame);
    assert_eq!(api.status_code.map(|s| s.as_u16()), Some(429));
    assert!(
        api.message.contains("Rate limit reached"),
        "{}",
        api.message
    );
    let fallback = early_error(&structure, url, &json!({"error": {}}));
    assert_eq!(fallback.status_code.map(|s| s.as_u16()), Some(500));
    assert_eq!(
        fallback.message,
        "stream failed before any output was generated"
    );
    assert!(!fallback.is_retryable);
}
