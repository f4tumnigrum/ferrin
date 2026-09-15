//! Error events obey recording flags without changing application errors.

use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::Error;
use ferrin_core::Output;
use ferrin_core::error::ErrorKind;
use ferrin_core::error::RetryReason;
use ferrin_core::generate_text;
use ferrin_core::output::OutputContext;
use ferrin_core::output::OutputHandler;
use ferrin_core::stream_text;
use ferrin_core::telemetry::ErrorEvent;
use ferrin_core::telemetry::Telemetry;
use ferrin_core::telemetry::TelemetryOptions;
use ferrin_spec::JsonValue;
use ferrin_spec::ResponseFormat;
use ferrin_spec::Usage;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::ProviderError;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

use super::common::mock;
use super::common::text_result;

type ErrorSummary = (ErrorKind, Option<u16>, bool, Option<(u32, usize)>, String);

#[derive(Default)]
struct ErrorRecorder(Mutex<Vec<ErrorSummary>>);
impl Telemetry for ErrorRecorder {
    fn on_error(&self, event: &ErrorEvent<'_>) {
        let mut rendered = format!("{:?} {}", event.error, event.error);
        let mut cause = std::error::Error::source(event.error);
        while let Some(source) = cause {
            rendered.push_str(&format!(" {source:?} {source}"));
            cause = source.source();
        }
        let attempts = match event.error {
            Error::Retry {
                attempts, errors, ..
            } => Some((*attempts, errors.len())),
            _ => None,
        };
        self.0.lock().unwrap().push((
            event.error.kind(),
            event.error.status_code().map(|status| status.as_u16()),
            event.error.is_retryable(),
            attempts,
            rendered,
        ));
    }
}

#[tokio::test]
async fn structured_output_error_events_remove_text_body_and_validation_causes() {
    for text in ["SECRET_OUTPUT", "{\"answer\":\"SECRET_OUTPUT\"}"] {
        for streaming in [false, true] {
            for record_outputs in [false, true] {
                let recorder = Arc::new(ErrorRecorder::default());
                let options = TelemetryOptions {
                    record_outputs,
                    record_inputs: record_outputs,
                    ..TelemetryOptions::enabled().with_integration(recorder.clone())
                };
                let mut response = text_result(text);
                response.response.body = Some(json!({"content":text}));
                let model = mock()
                    .generate(response)
                    .stream(ferrin_testing::text_parts([text], Usage::totals(2, 3)))
                    .build_shared();
                let output = Output::json_with_schema(
                    json!({"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"]}),
                );
                let error = if streaming {
                    stream_text(model)
                        .prompt("hi")
                        .telemetry(options)
                        .output(output)
                        .await
                        .unwrap()
                        .consume()
                        .await
                        .unwrap_err()
                } else {
                    generate_text(model)
                        .prompt("hi")
                        .telemetry(options)
                        .output(output)
                        .await
                        .unwrap_err()
                };
                let Error::NoObjectGenerated(details) = error else {
                    panic!("expected structured output error")
                };
                assert_eq!(details.text.as_deref(), Some(text));
                assert!(details.cause.is_some());
                let errors = recorder.0.lock().unwrap();
                assert_eq!(errors.len(), usize::from(streaming));
                for (kind, _, _, _, rendered) in errors.iter() {
                    assert_eq!(*kind, ErrorKind::Output);
                    assert_eq!(
                        rendered.contains("SECRET_OUTPUT"),
                        record_outputs,
                        "{rendered}"
                    );
                    assert!(rendered.contains("Usage"));
                }
            }
        }
    }
}

struct FailedOutput;
impl OutputHandler<()> for FailedOutput {
    fn response_format(&self) -> Option<ResponseFormat> {
        None
    }
    fn parse_complete(&self, _: &str, _: &OutputContext) -> Result<(), Error> {
        Err(Error::Retry {
            reason: RetryReason::MaxRetriesExceeded,
            attempts: 2,
            errors: (0..2)
                .map(|_| {
                    ProviderError::from(
                        ApiCallError::new(
                            "SECRET_OUTPUT",
                            Url::parse("https://example.com/api?input=SECRET_INPUT").unwrap(),
                        )
                        .with_status(http::StatusCode::TOO_MANY_REQUESTS)
                        .with_request_body(json!({"input":"SECRET_INPUT"}))
                        .with_response(Default::default(), Some("SECRET_OUTPUT".into()))
                        .with_data(json!({"output":"SECRET_OUTPUT"}))
                        .with_cause(std::io::Error::other("SECRET_CAUSE")),
                    )
                })
                .collect(),
        })
    }
    fn parse_partial(&self, _: &str) -> Option<JsonValue> {
        None
    }
}

#[tokio::test]
async fn retry_error_events_preserve_counts_status_and_retryability_without_bodies() {
    let recorder = Arc::new(ErrorRecorder::default());
    let error = stream_text(
        mock()
            .stream(ferrin_testing::text_parts(["done"], Usage::default()))
            .build_shared(),
    )
    .prompt("hi")
    .telemetry(TelemetryOptions::enabled().with_integration(recorder.clone()))
    .output(Output::custom(FailedOutput))
    .await
    .unwrap()
    .consume()
    .await
    .unwrap_err();
    assert!(format!("{error:?}").contains("SECRET_INPUT"));
    let errors = recorder.0.lock().unwrap();
    assert_eq!(errors.len(), 1);
    let (kind, status, retryable, attempts, rendered) = &errors[0];
    assert_eq!(
        (*kind, *status, *retryable, *attempts),
        (ErrorKind::Retry, Some(429), true, Some((2, 2)))
    );
    assert!(!rendered.contains("SECRET_"), "{rendered}");
}

#[tokio::test]
async fn provider_stream_error_events_remove_raw_payloads() {
    let recorder = Arc::new(ErrorRecorder::default());
    let mut error = ferrin_spec::language_model::StreamError::new("SECRET_OUTPUT");
    error.data = Some(json!({"response":"SECRET_OUTPUT"}));
    error.status_code = Some(400);
    let model = mock()
        .stream(vec![
            ferrin_spec::StreamPart::stream_start(),
            ferrin_spec::StreamPart::Error { error },
        ])
        .build_shared();
    let original = stream_text(model)
        .prompt("hi")
        .telemetry(TelemetryOptions::enabled().with_integration(recorder.clone()))
        .await
        .unwrap()
        .consume()
        .await
        .unwrap_err();
    assert!(format!("{original:?}").contains("SECRET_OUTPUT"));
    let errors = recorder.0.lock().unwrap();
    assert!(!errors.is_empty());
    for (kind, status, _, _, rendered) in errors.iter() {
        assert_eq!((*kind, *status), (ErrorKind::Provider, Some(400)));
        assert!(!rendered.contains("SECRET_OUTPUT"), "{rendered}");
    }
}
