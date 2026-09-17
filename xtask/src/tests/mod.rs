//! Tests requiring access to the recording command's private helpers.

#![allow(
    clippy::unwrap_used,
    reason = "test code may panic on unexpected values"
)]

use crate::record_fixture::redact;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn response_redaction_preserves_unrelated_values_and_nulls() {
    let mut value =
        json!({"response": {"instructions": "private", "user": null}, "output": ["pong"]});
    assert!(redact::json(
        &mut value,
        &[
            "/response/instructions".into(),
            "/response/user".into(),
            "/missing".into()
        ]
    ));
    assert_eq!(
        value,
        json!({"response": {"instructions": "[REDACTED]", "user": null}, "output": ["pong"]})
    );
}

#[test]
fn sse_redaction_handles_multiline_json_and_preserves_event_fields() {
    let event = "event: response.created\nid: 7\n: comment\ndata: {\"response\":\ndata: {\"instructions\":\"private\"},\"type\":\"response.created\"}\nretry: 500";
    assert_eq!(
        redact::sse_event(event, &["/response/instructions".into()]).unwrap(),
        "event: response.created\nid: 7\n: comment\ndata: {\"response\":{\"instructions\":\"[REDACTED]\"},\"type\":\"response.created\"}\nretry: 500"
    );
}

#[test]
fn sse_redaction_preserves_unmatched_events_comments_and_done() {
    for event in [
        "event: delta\ndata: { \"delta\": \"pong\" }",
        ": ping",
        "data: [DONE]",
    ] {
        assert_eq!(
            redact::sse_event(event, &["/response/instructions".into()]).unwrap(),
            event
        );
    }
}

#[test]
fn sse_redaction_rejects_unparsable_payloads_when_enabled() {
    assert!(redact::sse_event("data: private", &["/user".into()]).is_err());
    assert_eq!(
        redact::sse_event("data: private", &[]).unwrap(),
        "data: private"
    );
}
