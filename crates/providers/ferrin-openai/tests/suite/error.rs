//! Error frame parsing shared by the streaming models.

use ferrin_openai::error::parse_frame_error;
use ferrin_openai::error::stream_error_for_frame;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

#[test]
fn numeric_codes_become_status_codes() {
    let frame = json!({"type": "error", "code": "429", "message": "slow down"});
    let error = parse_frame_error(&frame).unwrap();
    assert_eq!(error.status_code(), 429);
    assert!(error.is_retryable());
}

#[test]
fn discriminators_are_mapped_heuristically() {
    let cases = [
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
        let error = parse_frame_error(&frame).unwrap();
        assert_eq!(error.status_code(), status, "{frame}");
        assert_eq!(error.is_retryable(), retryable, "{frame}");
    }
}

#[test]
fn response_failed_frames_carry_the_nested_error() {
    let frame = json!({
        "type": "response.failed",
        "response": {"error": {"code": "rate_limit_exceeded", "message": "Rate limit reached"}}
    });
    let error = parse_frame_error(&frame).unwrap();
    assert_eq!(error.message, "Rate limit reached");
    let stream_error = stream_error_for_frame(&frame);
    assert_eq!(stream_error.status_code, Some(429));
    assert_eq!(stream_error.is_retryable, Some(true));
    let api = error.to_api_call_error(
        Url::parse("https://api.openai.com/v1/responses").unwrap(),
        &frame,
    );
    assert_eq!(api.status_code.map(|s| s.as_u16()), Some(429));
    assert!(api.message.contains("Rate limit reached"));
}

#[test]
fn frames_without_errors_yield_a_generic_stream_error() {
    assert!(parse_frame_error(&json!({"type": "response.created"})).is_none());
    let stream_error = stream_error_for_frame(&json!({"type": "weird"}));
    assert!(!stream_error.message.is_empty());
}
