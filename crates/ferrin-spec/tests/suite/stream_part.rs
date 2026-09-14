use pretty_assertions::assert_eq;
use serde_json::json;

use ferrin_spec::FinishReason;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::language_model::stream_part::StreamError;
use ferrin_spec::language_model::stream_part::StreamErrorCode;
use ferrin_spec::language_model::stream_part::StreamPart;

#[test]
fn stream_parts_round_trip() {
    let parts = vec![
        StreamPart::StreamStart {
            warnings: vec![Warning::unsupported("seed")],
        },
        StreamPart::ResponseMetadata {
            id: Some("resp_1".to_owned()),
            timestamp: Some("2026-09-13T00:00:00Z".parse().unwrap()),
            model_id: Some("gpt-5".into()),
        },
        StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        },
        StreamPart::text_delta("t1", "Hel"),
        StreamPart::TextEnd {
            id: "t1".into(),
            provider_metadata: None,
        },
        StreamPart::ToolInputStart {
            id: "call_1".into(),
            tool_name: "weather".into(),
            provider_executed: false,
            dynamic: true,
            title: None,
            provider_metadata: None,
        },
        StreamPart::finish(FinishReason::stop(), Usage::totals(1, 2)),
        StreamPart::Raw {
            raw_value: json!({ "x": 1 }),
        },
        StreamPart::Error {
            error: StreamError {
                message: "boom".to_owned(),
                error_type: Some("server_error".to_owned()),
                code: Some(StreamErrorCode::Number(500)),
                status_code: Some(500),
                is_retryable: Some(true),
                data: None,
            },
        },
    ];

    let value = serde_json::to_value(&parts).unwrap();
    assert_eq!(
        value,
        json!([
            { "type": "stream-start", "warnings": [{ "type": "unsupported", "feature": "seed" }] },
            { "type": "response-metadata", "id": "resp_1",
              "timestamp": "2026-09-13T00:00:00Z", "model_id": "gpt-5" },
            { "type": "text-start", "id": "t1" },
            { "type": "text-delta", "id": "t1", "delta": "Hel" },
            { "type": "text-end", "id": "t1" },
            { "type": "tool-input-start", "id": "call_1", "tool_name": "weather", "dynamic": true },
            { "type": "finish", "finish_reason": { "unified": "stop" },
              "usage": { "input": { "total": 1 }, "output": { "total": 2 } } },
            { "type": "raw", "raw_value": { "x": 1 } },
            { "type": "error", "error": { "message": "boom", "type": "server_error",
              "code": 500, "status_code": 500, "is_retryable": true } },
        ])
    );
    let parsed: Vec<StreamPart> = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, parts);
    assert_eq!(parsed[3].kind_name(), "text-delta");
}

#[test]
fn stream_error_code_accepts_text_or_number() {
    let text: StreamError =
        serde_json::from_value(json!({ "message": "m", "code": "E1" })).unwrap();
    assert_eq!(text.code, Some(StreamErrorCode::Text("E1".to_owned())));
    assert_eq!(text.code.unwrap().to_string(), "E1");
}
