use std::sync::Arc;

use ferrin_spec::Content;
use ferrin_spec::FinishReason;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::prompt::PromptMessage;
use ferrin_testing::MockCallKind;
use ferrin_testing::MockLanguageModel;
use ferrin_testing::api_call_error;
use ferrin_testing::text_parts;
use futures_util::StreamExt;
use http::StatusCode;
use pretty_assertions::assert_eq;

fn result(text: &str) -> GenerateResult {
    GenerateResult::new(vec![Content::text(text)], FinishReason::stop())
}

fn options(text: &str) -> CallOptions {
    CallOptions::new(vec![PromptMessage::user_text(text)])
}

#[tokio::test]
async fn queued_responses_are_consumed_in_order_then_exhausted() {
    let model = MockLanguageModel::builder()
        .provider("test-provider")
        .model_id("test-model")
        .generate(result("first"))
        .generate(result("second"))
        .build();
    assert_eq!(model.provider().as_str(), "test-provider");
    assert_eq!(model.model_id().as_str(), "test-model");

    let first = model.do_generate(options("a")).await.unwrap();
    assert_eq!(first.content[0].as_text(), Some("first"));
    let second = model.do_generate(options("b")).await.unwrap();
    assert_eq!(second.content[0].as_text(), Some("second"));

    let error = model.do_generate(options("c")).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "mock language model: no generate response scripted for call #3"
    );

    let calls = model.calls();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| call.kind == MockCallKind::Generate));
    assert_eq!(model.generate_calls().len(), 3);
    assert!(model.stream_calls().is_empty());
}

#[tokio::test]
async fn repeat_and_closure_fallbacks_answer_after_the_queue() {
    let model = MockLanguageModel::builder()
        .generate(result("queued"))
        .generate_repeat(result("repeated"))
        .build();
    assert_eq!(
        model.do_generate(options("a")).await.unwrap().content[0].as_text(),
        Some("queued")
    );
    for _ in 0..2 {
        assert_eq!(
            model.do_generate(options("b")).await.unwrap().content[0].as_text(),
            Some("repeated")
        );
    }

    let model = MockLanguageModel::builder()
        .generate_with(|options: CallOptions| async move {
            let text = options.prompt.len().to_string();
            Ok(result(&text))
        })
        .build();
    assert_eq!(
        model.do_generate(options("x")).await.unwrap().content[0].as_text(),
        Some("1")
    );
}

#[tokio::test]
async fn queued_errors_and_streams() {
    let model = Arc::new(
        MockLanguageModel::builder()
            .generate_error(api_call_error(StatusCode::TOO_MANY_REQUESTS, "slow down"))
            .stream(text_parts(["Hel", "lo"], Usage::totals(1, 2)))
            .stream_error(ProviderError::message("boom"))
            .build(),
    );
    let error = model.do_generate(options("a")).await.unwrap_err();
    assert!(error.is_retryable());
    assert_eq!(error.status_code(), Some(StatusCode::TOO_MANY_REQUESTS));

    let stream = model.do_stream(options("b")).await.unwrap();
    let parts: Vec<StreamPart> = stream.stream.collect().await;
    assert_eq!(parts.len(), 6);
    assert!(matches!(parts[0], StreamPart::StreamStart { .. }));
    assert!(matches!(parts[5], StreamPart::Finish { .. }));

    let error = model.do_stream(options("c")).await.unwrap_err();
    assert_eq!(error.to_string(), "boom");
    assert_eq!(model.stream_calls().len(), 2);
    assert_eq!(model.call_count(), 3);
}

#[tokio::test]
async fn streaming_shorthand_repeats_parts() {
    let model = MockLanguageModel::streaming(text_parts(["x"], Usage::totals(0, 1)));
    for _ in 0..2 {
        let stream = model.do_stream(options("a")).await.unwrap();
        assert_eq!(stream.stream.collect::<Vec<_>>().await.len(), 5);
    }
}
