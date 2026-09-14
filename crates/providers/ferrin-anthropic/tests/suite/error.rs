//! Error type mapping shared by responses and stream frames.

use ferrin_anthropic::error::FrameError;
use ferrin_anthropic::error::status_for_error_type;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

#[test]
fn error_types_map_to_status_codes_and_retryability() {
    let cases = [
        ("api_error", 500, true),
        ("overloaded_error", 529, true),
        ("rate_limit_error", 429, true),
        ("request_too_large", 413, false),
        ("authentication_error", 401, false),
        ("permission_error", 403, false),
        ("not_found_error", 404, false),
        ("billing_error", 400, false),
        ("invalid_request_error", 400, false),
    ];
    for (error_type, status, retryable) in cases {
        assert_eq!(
            status_for_error_type(Some(error_type)),
            (status, retryable),
            "{error_type}"
        );
    }
    assert_eq!(status_for_error_type(Some("mystery")), (500, false));
    assert_eq!(status_for_error_type(None), (500, false));
}

#[test]
fn frame_errors_convert_to_stream_and_api_call_errors() {
    let frame = json!({"type": "overloaded_error", "message": "Overloaded"});
    let error = FrameError::from_error_object(&frame);
    assert_eq!(error.message, "Overloaded");
    assert_eq!(error.error_type.as_deref(), Some("overloaded_error"));
    let stream_error = error.to_stream_error(&frame);
    assert_eq!(stream_error.status_code, Some(529));
    assert_eq!(stream_error.is_retryable, Some(true));
    assert_eq!(stream_error.data, Some(frame.clone()));
    let api_error = error.to_api_call_error(
        Url::parse("https://example.test/v1/messages").unwrap(),
        &frame,
    );
    assert_eq!(
        api_error.status_code.map(|status| status.as_u16()),
        Some(529)
    );
    assert!(api_error.is_retryable);
    assert_eq!(
        api_error.response_body.as_deref(),
        Some(frame.to_string().as_str())
    );

    let missing = FrameError::from_error_object(&json!({}));
    assert_eq!(missing.message, "Anthropic stream error");
    assert_eq!(missing.status_code(), 500);
    assert!(!missing.is_retryable());
}
