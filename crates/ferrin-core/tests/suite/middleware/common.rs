//! Helpers for exercising middleware directly on a wrapped model.

use std::sync::Arc;

use ferrin_core::LanguageModelMiddleware;
use ferrin_core::wrap_language_model;
use ferrin_spec::Content;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::FinishReason;
use ferrin_spec::PartId;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::prompt::PromptMessage;
use ferrin_testing::MockLanguageModel;
use futures_util::StreamExt;

pub(crate) fn options() -> CallOptions {
    CallOptions::new(vec![PromptMessage::user_text("hi")])
}

pub(crate) fn wrapped(
    model: Arc<MockLanguageModel>,
    middleware: impl LanguageModelMiddleware,
) -> Arc<dyn DynLanguageModel> {
    wrap_language_model(
        model as Arc<dyn DynLanguageModel>,
        [Arc::new(middleware) as Arc<dyn LanguageModelMiddleware>],
    )
}

pub(crate) fn text_result(text: &str) -> GenerateResult {
    GenerateResult::new(vec![Content::text(text)], FinishReason::stop())
}

/// Stream parts: `stream-start`, `text-start(id)`, the deltas, `text-end`,
/// `finish`.
pub(crate) fn text_stream(id: &str, deltas: &[&str]) -> Vec<StreamPart> {
    let id = PartId::new(id);
    let mut parts = vec![
        StreamPart::stream_start(),
        StreamPart::TextStart {
            id: id.clone(),
            provider_metadata: None,
        },
    ];
    parts.extend(
        deltas
            .iter()
            .map(|delta| StreamPart::text_delta(id.clone(), *delta)),
    );
    parts.push(StreamPart::TextEnd {
        id,
        provider_metadata: None,
    });
    parts.push(StreamPart::finish(
        FinishReason::stop(),
        Usage::totals(1, 1),
    ));
    parts
}

pub(crate) async fn generate(model: &Arc<dyn DynLanguageModel>) -> GenerateResult {
    model.do_generate(options()).await.unwrap()
}

pub(crate) async fn stream(model: &Arc<dyn DynLanguageModel>) -> Vec<StreamPart> {
    model
        .do_stream(options())
        .await
        .unwrap()
        .stream
        .collect()
        .await
}

/// Compact rendering of stream parts for assertions: `kind(id):payload`.
pub(crate) fn render(parts: &[StreamPart]) -> Vec<String> {
    parts
        .iter()
        .map(|part| match part {
            StreamPart::TextStart { id, .. } => format!("text-start({id})"),
            StreamPart::TextDelta { id, delta, .. } => format!("text-delta({id}):{delta}"),
            StreamPart::TextEnd { id, .. } => format!("text-end({id})"),
            StreamPart::ReasoningStart { id, .. } => format!("reasoning-start({id})"),
            StreamPart::ReasoningDelta { id, delta, .. } => {
                format!("reasoning-delta({id}):{delta}")
            }
            StreamPart::ReasoningEnd { id, .. } => format!("reasoning-end({id})"),
            other => other.kind_name().to_owned(),
        })
        .collect()
}

pub(crate) fn render_content(content: &[Content]) -> Vec<String> {
    content
        .iter()
        .map(|part| match part {
            Content::Text { text, .. } => format!("text:{text}"),
            Content::Reasoning { text, .. } => format!("reasoning:{text}"),
            other => other.kind_name().to_owned(),
        })
        .collect()
}
