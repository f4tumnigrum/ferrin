//! The `stream_text` builder.

use futures_util::FutureExt;
use std::fmt;
use std::future::Future;
use std::future::IntoFuture;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_spec::BoxFuture;
use ferrin_spec::LanguageModelRef;

use super::StreamErrorInfo;
use super::StreamEvent;
use super::pipeline;
use super::result::StreamTextResult;
use super::transforms::StreamTransform;
use crate::error::Error;
use crate::generate_text::config::CallConfig;
use crate::hooks::HookFn;
use crate::output::NoOutput;
use crate::output::Output;
use crate::output::OutputHandler;
use crate::telemetry::AbortEvent;

/// What the pipeline does after an error reported by the model stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ErrorDecision {
    /// Report the error and end the call.
    #[default]
    Continue,
    /// Retry the model call of the current step. Honoured at most once per
    /// step and only when [`StreamText::stream_retries`] is configured.
    Retry,
}

/// Callback invoked for every error that occurs while streaming.
///
/// Implemented for every `Fn(StreamErrorInfo) -> impl Future<Output =
/// ErrorDecision>` closure.
pub trait OnErrorFn: Send + Sync + 'static {
    /// Handles one error and decides how to proceed.
    fn call(&self, error: StreamErrorInfo) -> BoxFuture<'static, ErrorDecision>;
}

impl<F, Fut> OnErrorFn for F
where
    F: Fn(StreamErrorInfo) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ErrorDecision> + Send + 'static,
{
    fn call(&self, error: StreamErrorInfo) -> BoxFuture<'static, ErrorDecision> {
        Box::pin(self(error))
    }
}

/// Streaming-only settings.
#[derive(Default)]
pub(crate) struct StreamConfig {
    pub(crate) transforms: Vec<Arc<dyn StreamTransform>>,
    pub(crate) include_raw_chunks: bool,
    pub(crate) stream_retries: Option<u32>,
    pub(crate) on_error: Option<Arc<dyn OnErrorFn>>,
    handled_errors: Mutex<Vec<StreamErrorInfo>>,
}

impl StreamConfig {
    pub(crate) fn mark_error_handled(&self, error: StreamErrorInfo) {
        self.handled_errors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(error);
    }

    pub(crate) fn take_error_handled(&self, error: &StreamErrorInfo) -> bool {
        let mut handled = self
            .handled_errors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = handled.iter().position(|item| item == error) {
            handled.remove(index);
            true
        } else {
            false
        }
    }

    pub(crate) async fn error_decision(&self, error: StreamErrorInfo) -> ErrorDecision {
        let Some(callback) = &self.on_error else {
            return ErrorDecision::Continue;
        };
        let Ok(future) = catch_unwind(AssertUnwindSafe(|| callback.call(error))) else {
            return ErrorDecision::Continue;
        };
        AssertUnwindSafe(future)
            .catch_unwind()
            .await
            .unwrap_or(ErrorDecision::Continue)
    }
}

impl fmt::Debug for StreamConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamConfig")
            .field("transforms", &self.transforms.len())
            .field("include_raw_chunks", &self.include_raw_chunks)
            .field("stream_retries", &self.stream_retries)
            .field("on_error", &self.on_error.is_some())
            .finish()
    }
}

/// Starts building a streaming text generation call.
///
/// The builder is a future: `.await` starts the pipeline and resolves once
/// the first model request has been established (or failed after retries).
#[must_use]
pub fn stream_text(model: impl Into<LanguageModelRef>) -> StreamText<()> {
    StreamText {
        config: CallConfig::new(model.into()),
        output: Arc::new(NoOutput),
        stream: StreamConfig {
            transforms: Vec::new(),
            include_raw_chunks: false,
            stream_retries: None,
            on_error: None,
            handled_errors: Mutex::new(Vec::new()),
        },
    }
}

/// Builder and future of a `stream_text` call.
pub struct StreamText<O> {
    pub(crate) config: CallConfig,
    pub(crate) output: Arc<dyn OutputHandler<O>>,
    pub(crate) stream: StreamConfig,
}

crate::builder::impl_call_builder!(StreamText);

impl<O> StreamText<O> {
    /// Requests structured output parsed by `output`.
    pub fn output<T>(self, output: Output<T>) -> StreamText<T> {
        StreamText {
            config: self.config,
            output: output.handler(),
            stream: self.stream,
        }
    }

    /// Appends a transform applied to the event stream before the step
    /// results are accumulated. Transforms run in order.
    #[must_use]
    pub fn transform(mut self, transform: impl StreamTransform + 'static) -> Self {
        self.stream.transforms.push(Arc::new(transform));
        self
    }

    /// Forwards raw provider chunks as [`StreamEvent::Raw`].
    #[must_use]
    pub fn include_raw_chunks(mut self) -> Self {
        self.stream.include_raw_chunks = true;
        self
    }

    /// Enables stream-level retries: after an error reported by the model
    /// stream the current step is retried up to `retries` times. `0` allows
    /// only retries requested by the [`on_error`](Self::on_error) callback.
    /// Retries are disabled when this is not called.
    #[must_use]
    pub fn stream_retries(mut self, retries: u32) -> Self {
        self.stream.stream_retries = Some(retries);
        self
    }

    /// Observes transformed events and provider errors considered for retry.
    ///
    /// A provider error is observed before `on_error`, including errors
    /// swallowed by a successful retry. Retry boundaries bypass this hook;
    /// a terminal provider error is not reported twice after transforms.
    #[must_use]
    pub fn on_chunk(mut self, f: impl HookFn<StreamEvent>) -> Self {
        self.config.hooks.on_chunk.push(Arc::new(f));
        self
    }

    /// Runs when the call is aborted by cancellation.
    #[must_use]
    pub fn on_abort(mut self, f: impl HookFn<AbortEvent>) -> Self {
        self.config.hooks.on_abort.push(Arc::new(f));
        self
    }

    /// Runs for every error that occurs while streaming; the returned
    /// decision may request a retry (see [`stream_retries`](Self::stream_retries)).
    /// Synchronous and asynchronous callback panics are ignored, preserving
    /// the original provider failure and configured automatic retry budget.
    #[must_use]
    pub fn on_error(mut self, f: impl OnErrorFn) -> Self {
        self.stream.on_error = Some(Arc::new(f));
        self
    }
}

impl<O> fmt::Debug for StreamText<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamText")
            .field("config", &self.config)
            .field("stream", &self.stream)
            .finish_non_exhaustive()
    }
}

impl<O: Send + 'static> IntoFuture for StreamText<O> {
    type Output = Result<StreamTextResult<O>, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.output.validate_configuration()?;
            pipeline::start(self.config, self.output, self.stream).await
        })
    }
}
