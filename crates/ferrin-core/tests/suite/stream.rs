use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_core::Error;
use ferrin_core::Output;
use ferrin_core::StreamEvent;
use ferrin_core::Timeout;
use ferrin_core::step_count;
use ferrin_core::stream_text;
use ferrin_core::stream_text::ErrorDecision;
use ferrin_core::stream_text::EventStream;
use ferrin_core::stream_text::StreamErrorInfo;
use ferrin_core::stream_text::TransformContext;
use ferrin_core::stream_text::smooth_stream;
use ferrin_core::stream_text::transforms::Chunking;
use ferrin_core::stream_text::transforms::SmoothStreamConfig;
use ferrin_core::timeout::TimeoutScope;
use ferrin_schema::schemars;
use ferrin_spec::FinishReason;
use ferrin_spec::PartId;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolCallId;
use ferrin_spec::Usage;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::StreamError;
use ferrin_testing::SimulatedStream;
use ferrin_testing::text_parts;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::json;

use super::common::kinds;
use super::common::mock;
use super::common::weather_tools;

fn event_kinds(events: &[StreamEvent]) -> Vec<&'static str> {
    events.iter().map(StreamEvent::kind_name).collect()
}

fn text_deltas(events: &[StreamEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn tool_call_parts() -> Vec<StreamPart> {
    vec![
        StreamPart::stream_start(),
        StreamPart::ToolInputStart {
            id: ToolCallId::new("call-1"),
            tool_name: "get_weather".into(),
            provider_executed: false,
            dynamic: false,
            title: None,
            provider_metadata: None,
        },
        StreamPart::ToolInputDelta {
            id: ToolCallId::new("call-1"),
            delta: "{\"city\":".to_owned(),
            provider_metadata: None,
        },
        StreamPart::ToolInputDelta {
            id: ToolCallId::new("call-1"),
            delta: "\"Oslo\"}".to_owned(),
            provider_metadata: None,
        },
        StreamPart::ToolInputEnd {
            id: ToolCallId::new("call-1"),
            provider_metadata: None,
        },
        StreamPart::ToolCall(ToolCall::new(
            "call-1",
            "get_weather",
            "{\"city\":\"Oslo\"}",
        )),
        StreamPart::finish(FinishReason::tool_calls(), Usage::totals(5, 5)),
    ]
}

#[tokio::test]
async fn streams_text_events_and_resolves_the_completion() {
    let model = mock()
        .stream(text_parts(["Hel", "lo"], Usage::totals(3, 2)))
        .build_shared();
    let result = stream_text(Arc::clone(&model)).prompt("hi").await.unwrap();
    assert!(!result.call_id().is_empty());
    let (events, completion) = result.split();
    let events: Vec<StreamEvent> = events.collect().await;
    assert_eq!(
        event_kinds(&events),
        vec![
            "start",
            "start-step",
            "text-start",
            "text-delta",
            "text-delta",
            "text-end",
            "finish-step",
            "finish",
        ]
    );
    assert_eq!(text_deltas(&events), vec!["Hel", "lo"]);
    let result = completion.await.unwrap();
    assert_eq!(result.text(), "Hello");
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.total_usage.total_tokens(), Some(5));
    assert_eq!(result.last_step().response.messages.len(), 1);
}

#[tokio::test]
async fn text_stream_yields_deltas() {
    let model = mock()
        .stream(text_parts(["a", "b", "c"], Usage::totals(1, 1)))
        .build_shared();
    let result = stream_text(model).prompt("hi").await.unwrap();
    let chunks: Vec<String> = result
        .text_stream()
        .map(|chunk| chunk.unwrap())
        .collect()
        .await;
    assert_eq!(chunks, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn streamed_tool_calls_are_executed_and_the_loop_continues() {
    let model = mock()
        .stream(tool_call_parts())
        .stream(text_parts(["Sunny"], Usage::totals(2, 2)))
        .build_shared();
    let result = stream_text(Arc::clone(&model))
        .prompt("hi")
        .tools(weather_tools())
        .stop_when(step_count(5))
        .await
        .unwrap();
    let (events, completion) = result.split();
    let events: Vec<StreamEvent> = events.collect().await;
    assert_eq!(
        event_kinds(&events),
        vec![
            "start",
            "start-step",
            "tool-input-start",
            "tool-input-delta",
            "tool-input-delta",
            "tool-input-end",
            "tool-call",
            "tool-result",
            "finish-step",
            "start-step",
            "text-start",
            "text-delta",
            "text-end",
            "finish-step",
            "finish",
        ]
    );
    let result = completion.await.unwrap();
    assert_eq!(result.steps.len(), 2);
    assert_eq!(
        kinds(&result.steps[0].content),
        vec!["tool-call", "tool-result"]
    );
    assert_eq!(result.text(), "Sunny");
    assert_eq!(model.stream_calls().len(), 2);
    assert_eq!(model.stream_calls()[1].prompt.len(), 3);
}

#[tokio::test]
async fn error_parts_end_the_stream_with_an_error_event() {
    let parts = vec![
        StreamPart::stream_start(),
        StreamPart::TextStart {
            id: PartId::new("0"),
            provider_metadata: None,
        },
        StreamPart::text_delta(PartId::new("0"), "partial"),
        StreamPart::Error {
            error: StreamError::new("upstream failed"),
        },
    ];
    let model = mock().stream(parts).build_shared();
    let (events, completion) = stream_text(Arc::clone(&model))
        .prompt("hi")
        .await
        .unwrap()
        .split();
    let events: Vec<StreamEvent> = events.collect().await;
    assert_eq!(
        event_kinds(&events),
        vec!["start", "start-step", "text-start", "text-delta", "error"]
    );
    match &events[4] {
        StreamEvent::Error { error } => assert_eq!(error.message, "stream error: upstream failed"),
        other => panic!("unexpected event {other:?}"),
    }
    let error = completion.await.unwrap_err();
    assert!(matches!(error, Error::Stream(_)), "{error:?}");
    assert_eq!(model.stream_calls().len(), 1);
}

#[tokio::test]
async fn retryable_error_parts_restart_the_model_call() {
    let mut failing = StreamError::new("hiccup");
    failing.is_retryable = Some(true);
    let first = vec![
        StreamPart::stream_start(),
        StreamPart::TextStart {
            id: PartId::new("0"),
            provider_metadata: None,
        },
        StreamPart::text_delta(PartId::new("0"), "bad"),
        StreamPart::Error { error: failing },
    ];
    let model = mock()
        .stream(first)
        .stream(text_parts(["good"], Usage::totals(1, 1)))
        .build_shared();
    let (events, completion) = stream_text(Arc::clone(&model))
        .prompt("hi")
        .stream_retries(1)
        .await
        .unwrap()
        .split();
    let events: Vec<StreamEvent> = events.collect().await;
    assert_eq!(
        event_kinds(&events),
        vec![
            "start",
            "start-step",
            "text-start",
            "text-delta",
            "text-end",
            "retry-attempt",
            "text-start",
            "text-delta",
            "text-end",
            "finish-step",
            "finish",
        ]
    );
    match &events[5] {
        StreamEvent::RetryAttempt {
            step_number,
            attempt,
            ..
        } => {
            assert_eq!(*step_number, 0);
            assert_eq!(*attempt, 1);
        }
        other => panic!("unexpected event {other:?}"),
    }
    let result = completion.await.unwrap();
    assert_eq!(result.text(), "good");
    assert_eq!(model.stream_calls().len(), 2);
}

#[tokio::test]
async fn on_error_callback_can_request_a_retry() {
    let first = vec![
        StreamPart::stream_start(),
        StreamPart::Error {
            error: StreamError::new("not retryable by itself"),
        },
    ];
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let model = mock()
        .stream(first)
        .stream(text_parts(["ok"], Usage::totals(1, 1)))
        .build_shared();
    let result = stream_text(Arc::clone(&model))
        .prompt("hi")
        .stream_retries(0)
        .on_error({
            let seen = Arc::clone(&seen);
            move |info: StreamErrorInfo| {
                seen.lock().unwrap().push(info.message);
                async { ErrorDecision::Retry }
            }
        })
        .await
        .unwrap()
        .consume()
        .await
        .unwrap();
    assert_eq!(result.text(), "ok");
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(model.stream_calls().len(), 2);
}

#[tokio::test]
async fn streams_without_a_finish_part_are_invalid() {
    let parts = vec![
        StreamPart::stream_start(),
        StreamPart::text_delta(PartId::new("0"), "x"),
    ];
    let error = stream_text(mock().stream(parts).build_shared())
        .prompt("hi")
        .await
        .unwrap()
        .consume()
        .await
        .unwrap_err();
    assert!(
        matches!(error, Error::InvalidStreamPart { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn failing_do_stream_is_returned_from_the_initial_await() {
    let model = mock()
        .stream_error(ferrin_testing::api_call_error(
            http::StatusCode::BAD_REQUEST,
            "bad",
        ))
        .build_shared();
    let error = stream_text(model).prompt("hi").await.unwrap_err();
    assert!(matches!(error, Error::Provider(_)), "{error:?}");
}

#[tokio::test(start_paused = true)]
async fn first_chunk_timeout_aborts_the_stream() {
    let model = mock()
        .stream_with(|_: CallOptions| async {
            Ok(
                SimulatedStream::new(text_parts(["late"], Usage::totals(1, 1)))
                    .initial_delay(Duration::from_secs(30))
                    .build(),
            )
        })
        .build_shared();
    let (events, completion) = stream_text(model)
        .prompt("hi")
        .timeout(Timeout::none().with_first_chunk(Duration::from_secs(2)))
        .await
        .unwrap()
        .split();
    let events: Vec<StreamEvent> = events.collect().await;
    assert_eq!(event_kinds(&events), vec!["start", "error"]);
    match completion.await.unwrap_err() {
        Error::Timeout { scope, .. } => assert_eq!(scope, TimeoutScope::FirstChunk),
        other => panic!("unexpected error {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn chunk_timeout_aborts_between_parts() {
    let model = mock()
        .stream_with(|_: CallOptions| async {
            Ok(
                SimulatedStream::new(text_parts(["a", "b"], Usage::totals(1, 1)))
                    .chunk_delay(Duration::from_secs(10))
                    .build(),
            )
        })
        .build_shared();
    let error = stream_text(model)
        .prompt("hi")
        .timeout(Timeout::none().with_chunk(Duration::from_secs(1)))
        .await
        .unwrap()
        .consume()
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Timeout {
                scope: TimeoutScope::Chunk,
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn smooth_stream_rechunks_text_by_words_and_lines() {
    let model = mock()
        .stream(text_parts(
            ["Hel", "lo wor", "ld! By", "e"],
            Usage::totals(1, 1),
        ))
        .build_shared();
    let (events, completion) = stream_text(Arc::clone(&model))
        .prompt("hi")
        .transform(smooth_stream(SmoothStreamConfig::new().delay(None)))
        .await
        .unwrap()
        .split();
    let events: Vec<StreamEvent> = events.collect().await;
    assert_eq!(text_deltas(&events), vec!["Hello ", "world! ", "Bye"]);
    assert_eq!(completion.await.unwrap().text(), "Hello world! Bye");

    let model = mock()
        .stream(text_parts(
            ["line one\nli", "ne two\n", "tail"],
            Usage::totals(1, 1),
        ))
        .build_shared();
    let events: Vec<StreamEvent> = stream_text(model)
        .prompt("hi")
        .transform(smooth_stream(
            SmoothStreamConfig::new()
                .delay(None)
                .chunking(Chunking::Line),
        ))
        .await
        .unwrap()
        .split()
        .0
        .collect()
        .await;
    assert_eq!(
        text_deltas(&events),
        vec!["line one\n", "line two\n", "tail"]
    );
}

#[tokio::test]
async fn transforms_can_stop_the_stream() {
    let model = mock()
        .stream(text_parts(["a", "b", "c"], Usage::totals(1, 1)))
        .build_shared();
    let stop_after_first = |input: EventStream, ctx: TransformContext| -> EventStream {
        let mut seen = 0usize;
        Box::pin(input.map(move |event| {
            if matches!(event, StreamEvent::TextDelta { .. }) {
                seen += 1;
                if seen == 1 {
                    ctx.stop();
                }
            }
            event
        }))
    };
    let (events, completion) = stream_text(model)
        .prompt("hi")
        .transform(stop_after_first)
        .await
        .unwrap()
        .split();
    let events: Vec<StreamEvent> = events.collect().await;
    // The event that triggered the stop passes through; nothing follows.
    assert_eq!(
        event_kinds(&events),
        vec!["start", "start-step", "text-start", "text-delta"]
    );
    let error = completion.await.unwrap_err();
    assert!(error.is_cancelled(), "{error:?}");
}

#[derive(Debug, Deserialize, PartialEq, schemars::JsonSchema)]
struct Weather {
    city: String,
    temperature: i64,
}

#[tokio::test]
async fn partial_and_element_streams_follow_the_text() {
    let model = mock()
        .stream(text_parts(
            ["{\"city\":\"A\"", ",\"temperature\":2}"],
            Usage::totals(1, 1),
        ))
        .build_shared();
    let partials: Vec<_> = stream_text(model)
        .prompt("hi")
        .output(Output::<Weather>::object())
        .await
        .unwrap()
        .partial_output_stream()
        .collect()
        .await;
    assert_eq!(
        partials.iter().map(|p| p.value.clone()).collect::<Vec<_>>(),
        vec![
            json!({ "city": "A" }),
            json!({ "city": "A", "temperature": 2 })
        ]
    );
    assert_eq!(partials[0].typed, None);
    assert_eq!(
        partials[1].typed,
        Some(Weather {
            city: "A".to_owned(),
            temperature: 2,
        })
    );

    let model = mock()
        .stream(text_parts(
            [
                "{\"elements\":[{\"city\":\"A\",\"temperature\":1}",
                ",{\"city\":\"B\",\"temperature\":2}]}",
            ],
            Usage::totals(1, 1),
        ))
        .build_shared();
    let elements: Vec<Weather> = stream_text(model)
        .prompt("hi")
        .output(Output::<Vec<Weather>>::array())
        .await
        .unwrap()
        .element_stream()
        .map(|element| element.unwrap())
        .collect()
        .await;
    assert_eq!(
        elements.iter().map(|w| w.city.as_str()).collect::<Vec<_>>(),
        vec!["A", "B"]
    );
}
