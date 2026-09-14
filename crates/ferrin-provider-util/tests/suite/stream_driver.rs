use ferrin_provider_util::ParseResult;
use ferrin_provider_util::stream_driver::EarlyChunk;
use ferrin_provider_util::stream_driver::StreamMachine;
use ferrin_provider_util::stream_driver::drive_stream;
use ferrin_provider_util::stream_driver::fail_on_early_error;
use ferrin_spec::FinishReason;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use futures_util::StreamExt;
use futures_util::stream;
use pretty_assertions::assert_eq;
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Chunk {
    Accepted,
    Text(&'static str),
    Error(&'static str),
}

fn ok(chunk: Chunk) -> ParseResult<Chunk> {
    let raw = json!({"chunk": format!("{chunk:?}")});
    ParseResult::Ok { value: chunk, raw }
}

fn classify(chunk: &Chunk) -> EarlyChunk {
    match chunk {
        Chunk::Accepted => EarlyChunk::Accepted,
        Chunk::Text(_) => EarlyChunk::Output,
        Chunk::Error(_) => EarlyChunk::Error,
    }
}

fn to_error(chunk: &Chunk, _raw: &serde_json::Value) -> ProviderError {
    UnsupportedFunctionalityError::new(format!("early {chunk:?}")).into()
}

async fn values(stream: ferrin_spec::BoxStream<'static, ParseResult<Chunk>>) -> Vec<Chunk> {
    stream
        .filter_map(|chunk| async move { chunk.into_result().ok() })
        .collect()
        .await
}

#[tokio::test]
async fn early_error_fails_the_request() {
    let chunks = stream::iter(vec![ok(Chunk::Accepted), ok(Chunk::Error("boom"))]).boxed();
    let Err(error) = fail_on_early_error(chunks, classify, to_error).await else {
        panic!("expected an error");
    };
    assert!(error.to_string().contains("boom"), "{error}");
}

#[tokio::test]
async fn buffered_chunks_are_replayed_before_the_rest() {
    let chunks = stream::iter(vec![
        ok(Chunk::Accepted),
        ok(Chunk::Text("a")),
        ok(Chunk::Text("b")),
        ok(Chunk::Error("late")),
    ])
    .boxed();
    let replayed = fail_on_early_error(chunks, classify, to_error)
        .await
        .map_err(|error| error.to_string())
        .unwrap();
    assert_eq!(
        values(replayed).await,
        vec![
            Chunk::Accepted,
            Chunk::Text("a"),
            Chunk::Text("b"),
            Chunk::Error("late"),
        ]
    );
}

#[tokio::test]
async fn accepted_without_output_hands_over_after_the_grace_period() {
    let chunks = stream::iter(vec![ok(Chunk::Accepted)])
        .chain(stream::pending())
        .boxed();
    let replayed = fail_on_early_error(chunks, classify, to_error)
        .await
        .map_err(|error| error.to_string())
        .unwrap();
    let first = replayed.take(1).collect::<Vec<_>>().await;
    assert!(matches!(
        first.as_slice(),
        [ParseResult::Ok {
            value: Chunk::Accepted,
            ..
        }]
    ));
}

struct Machine {
    open: bool,
}

impl StreamMachine for Machine {
    type Chunk = Chunk;

    fn handle(&mut self, chunk: ParseResult<Chunk>, include_raw: bool) -> Vec<StreamPart> {
        let mut parts = Vec::new();
        let chunk = match chunk {
            ParseResult::Ok { value, raw } => {
                if include_raw {
                    parts.push(StreamPart::Raw { raw_value: raw });
                }
                value
            }
            ParseResult::Err { error, .. } => {
                parts.push(StreamPart::error(&error));
                return parts;
            }
        };
        match chunk {
            Chunk::Accepted => {}
            Chunk::Text(text) => {
                if !self.open {
                    self.open = true;
                    parts.push(StreamPart::TextStart {
                        id: "0".into(),
                        provider_metadata: None,
                    });
                }
                parts.push(StreamPart::text_delta("0", text));
            }
            Chunk::Error(message) => {
                parts.push(StreamPart::error(&ProviderError::message(message)));
                parts.push(StreamPart::text_delta("0", "dropped"));
            }
        }
        parts
    }

    fn finish(self) -> Vec<StreamPart> {
        let mut parts = Vec::new();
        if self.open {
            parts.push(StreamPart::TextEnd {
                id: "0".into(),
                provider_metadata: None,
            });
        }
        parts.push(StreamPart::finish(FinishReason::stop(), Usage::default()));
        parts
    }
}

fn kinds(parts: &[StreamPart]) -> Vec<&'static str> {
    parts.iter().map(StreamPart::kind_name).collect()
}

#[tokio::test]
async fn drive_stream_emits_start_machine_output_and_finish() {
    let chunks = stream::iter(vec![ok(Chunk::Text("a")), ok(Chunk::Text("b"))]).boxed();
    let parts: Vec<StreamPart> = drive_stream(
        StreamPart::stream_start(),
        chunks,
        Machine { open: false },
        true,
    )
    .collect()
    .await;
    assert_eq!(
        kinds(&parts),
        vec![
            "stream-start",
            "raw",
            "text-start",
            "text-delta",
            "raw",
            "text-delta",
            "text-end",
            "finish",
        ]
    );
}

#[tokio::test]
async fn error_part_closes_open_parts_and_terminates_the_stream() {
    let chunks = stream::iter(vec![
        ok(Chunk::Text("a")),
        ok(Chunk::Error("boom")),
        ok(Chunk::Text("never")),
    ])
    .boxed();
    let parts: Vec<StreamPart> = drive_stream(
        StreamPart::stream_start(),
        chunks,
        Machine { open: false },
        false,
    )
    .collect()
    .await;
    assert_eq!(
        kinds(&parts),
        vec![
            "stream-start",
            "text-start",
            "text-delta",
            "text-end",
            "error",
        ]
    );
}
