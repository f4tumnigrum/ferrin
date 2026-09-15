use std::time::Duration;

use bytes::Bytes;
use ferrin_provider_util::SseDecoder;
use ferrin_provider_util::SseEvent;
use ferrin_provider_util::sse::SseError;
use ferrin_provider_util::sse::decode_stream;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;

fn event(data: &str) -> SseEvent {
    SseEvent {
        event: None,
        data: data.to_owned(),
        id: None,
        retry: None,
        received_at: None,
    }
}

#[test]
fn decodes_fields_and_multi_line_data() {
    let mut decoder = SseDecoder::new();
    let events = decoder
        .feed(
            b"event: delta\nid: 7\nretry: 250\ndata: first\ndata: second\n\n: comment\ndata:x\n\n",
        )
        .unwrap();
    assert_eq!(
        events,
        vec![
            SseEvent {
                event: Some("delta".to_owned()),
                data: "first\nsecond".to_owned(),
                id: Some("7".to_owned()),
                retry: Some(Duration::from_millis(250)),
                received_at: None,
            },
            SseEvent {
                id: Some("7".to_owned()),
                retry: Some(Duration::from_millis(250)),
                ..event("x")
            },
        ]
    );
}

#[test]
fn handles_split_chunks_crlf_and_bom() {
    let mut decoder = SseDecoder::new();
    let mut events = Vec::new();
    for chunk in [
        &b"\xEF\xBB\xBFdata: a"[..],
        b"b\r",
        b"\ndata: c\r\r\n",
        b"data: d\n\n",
    ] {
        events.extend(decoder.feed(chunk).unwrap());
    }
    assert_eq!(events, vec![event("ab\nc"), event("d")]);
}

#[test]
fn ignores_events_without_data_and_unknown_fields() {
    let mut decoder = SseDecoder::new();
    let events = decoder.feed(b"event: ping\nfoo: bar\n\ndata\n\n").unwrap();
    assert_eq!(events, vec![event("")]);
}

#[test]
fn rejects_oversized_events() {
    let mut decoder = SseDecoder::new().with_max_event_bytes(8);
    assert_eq!(
        decoder.feed(b"data: 123456789\n\n"),
        Err(SseError::EventTooLarge { limit: 8 })
    );
}

#[tokio::test]
async fn stream_decoder_stamps_time_and_drops_incomplete_tail() {
    let body = futures_util::stream::iter(vec![
        Ok(Bytes::from_static(b"data: one\n\ndata: two\n")),
        Ok(Bytes::from_static(b"\ndata: partial")),
    ]);
    let events: Vec<_> = decode_stream(Box::pin(body), 1024).collect().await;
    let data: Vec<String> = events
        .into_iter()
        .map(|item| {
            let event = item.unwrap();
            assert!(event.received_at.is_some());
            event.data
        })
        .collect();
    assert_eq!(data, vec!["one".to_owned(), "two".to_owned()]);
}

#[test]
fn split_bom_and_line_boundaries_preserve_first_event() {
    for input in [
        &b"\xEF\xBB\xBFdata: first\r\n\r\ndata: next\n\n"[..],
        &b"data: first\r\n\r\ndata: next\n\n"[..],
    ] {
        for first in 0..=input.len() {
            for second in first..=input.len() {
                let mut decoder = SseDecoder::new();
                let mut events = Vec::new();
                for chunk in [&input[..first], &input[first..second], &input[second..]] {
                    events.extend(decoder.feed(chunk).unwrap());
                }
                assert_eq!(events, vec![event("first"), event("next")]);
            }
        }
    }
}

#[test]
fn incomplete_bom_prefix_does_not_swallow_line_terminator() {
    for prefix in [&b"\xEF"[..], &b"\xEF\xBB"[..]] {
        let mut decoder = SseDecoder::new();
        assert_eq!(decoder.feed(prefix).unwrap(), Vec::<SseEvent>::new());
        assert_eq!(decoder.feed(b"\ndata: ok\n\n").unwrap(), vec![event("ok")]);
    }
}
