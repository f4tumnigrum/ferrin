//! A scripted [`LanguageModel`].

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_spec::BoxFuture;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::StreamPart;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::StreamResult;
use ferrin_spec::language_model::SupportedUrls;
use http::StatusCode;
use url::Url;

use crate::stream::simulate_stream;

/// Which entry point a recorded call went through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MockCallKind {
    /// `do_generate`.
    Generate,
    /// `do_stream`.
    Stream,
}

/// One call received by a [`MockLanguageModel`].
#[derive(Debug, Clone)]
pub struct RecordedCall {
    /// Entry point.
    pub kind: MockCallKind,
    /// The options the model received.
    pub options: CallOptions,
}

type GenerateFn = Arc<
    dyn Fn(CallOptions) -> BoxFuture<'static, Result<GenerateResult, ProviderError>> + Send + Sync,
>;
type StreamFn = Arc<
    dyn Fn(CallOptions) -> BoxFuture<'static, Result<StreamResult, ProviderError>> + Send + Sync,
>;

/// A language model that replays scripted responses and records its calls.
///
/// Responses added with [`MockLanguageModelBuilder::generate`] and
/// [`MockLanguageModelBuilder::stream`] are consumed in order; once the queue
/// is empty the fallback closure (`generate_with`/`stream_with`) or repeated
/// response (`generate_repeat`/`stream_repeat`) answers. Without either, the
/// call fails with a [`ProviderError`] naming the call number.
pub struct MockLanguageModel {
    provider: ProviderId,
    model_id: ModelId,
    supported_urls: SupportedUrls,
    generate_queue: Mutex<VecDeque<Result<GenerateResult, ProviderError>>>,
    generate_fallback: Option<GenerateFn>,
    stream_queue: Mutex<VecDeque<Result<Vec<StreamPart>, ProviderError>>>,
    stream_fallback: Option<StreamFn>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl fmt::Debug for MockLanguageModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockLanguageModel")
            .field("provider", &self.provider)
            .field("model_id", &self.model_id)
            .field("calls", &self.call_count())
            .finish_non_exhaustive()
    }
}

impl MockLanguageModel {
    /// Starts building a mock with provider `mock` and model id `mock-model`.
    #[must_use]
    pub fn builder() -> MockLanguageModelBuilder {
        MockLanguageModelBuilder::default()
    }

    /// A mock answering every generate call with `result`.
    #[must_use]
    pub fn generating(result: GenerateResult) -> Self {
        Self::builder().generate_repeat(result).build()
    }

    /// A mock answering every stream call with `parts`.
    #[must_use]
    pub fn streaming(parts: Vec<StreamPart>) -> Self {
        Self::builder().stream_repeat(parts).build()
    }

    /// All calls received so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<RecordedCall> {
        lock(&self.calls).clone()
    }

    /// Number of calls received so far.
    #[must_use]
    pub fn call_count(&self) -> usize {
        lock(&self.calls).len()
    }

    /// Options of the generate calls received so far.
    #[must_use]
    pub fn generate_calls(&self) -> Vec<CallOptions> {
        self.calls_of(MockCallKind::Generate)
    }

    /// Options of the stream calls received so far.
    #[must_use]
    pub fn stream_calls(&self) -> Vec<CallOptions> {
        self.calls_of(MockCallKind::Stream)
    }

    fn calls_of(&self, kind: MockCallKind) -> Vec<CallOptions> {
        lock(&self.calls)
            .iter()
            .filter(|call| call.kind == kind)
            .map(|call| call.options.clone())
            .collect()
    }

    fn record(&self, kind: MockCallKind, options: &CallOptions) -> usize {
        let mut calls = lock(&self.calls);
        calls.push(RecordedCall {
            kind,
            options: options.clone(),
        });
        calls.len()
    }

    fn exhausted(kind: MockCallKind, call_number: usize) -> ProviderError {
        let entry = match kind {
            MockCallKind::Generate => "generate",
            MockCallKind::Stream => "stream",
        };
        ProviderError::message(format!(
            "mock language model: no {entry} response scripted for call #{call_number}"
        ))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl LanguageModel for MockLanguageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn supported_urls(&self) -> impl Future<Output = SupportedUrls> + Send {
        std::future::ready(self.supported_urls.clone())
    }

    fn do_generate(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<GenerateResult, ProviderError>> + Send {
        let call_number = self.record(MockCallKind::Generate, &options);
        let queued = lock(&self.generate_queue).pop_front();
        let fallback = self.generate_fallback.clone();
        async move {
            match queued {
                Some(result) => result,
                None => match fallback {
                    Some(function) => function(options).await,
                    None => Err(Self::exhausted(MockCallKind::Generate, call_number)),
                },
            }
        }
    }

    fn do_stream(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<StreamResult, ProviderError>> + Send {
        let call_number = self.record(MockCallKind::Stream, &options);
        let queued = lock(&self.stream_queue).pop_front();
        let fallback = self.stream_fallback.clone();
        async move {
            match queued {
                Some(result) => result.map(simulate_stream),
                None => match fallback {
                    Some(function) => function(options).await,
                    None => Err(Self::exhausted(MockCallKind::Stream, call_number)),
                },
            }
        }
    }
}

/// Builder for [`MockLanguageModel`].
pub struct MockLanguageModelBuilder {
    provider: ProviderId,
    model_id: ModelId,
    supported_urls: SupportedUrls,
    generate_queue: VecDeque<Result<GenerateResult, ProviderError>>,
    generate_fallback: Option<GenerateFn>,
    stream_queue: VecDeque<Result<Vec<StreamPart>, ProviderError>>,
    stream_fallback: Option<StreamFn>,
}

impl fmt::Debug for MockLanguageModelBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockLanguageModelBuilder")
            .field("provider", &self.provider)
            .field("model_id", &self.model_id)
            .field("generate_queue", &self.generate_queue.len())
            .field("stream_queue", &self.stream_queue.len())
            .finish_non_exhaustive()
    }
}

impl Default for MockLanguageModelBuilder {
    fn default() -> Self {
        Self {
            provider: ProviderId::new("mock"),
            model_id: ModelId::new("mock-model"),
            supported_urls: SupportedUrls::none(),
            generate_queue: VecDeque::new(),
            generate_fallback: None,
            stream_queue: VecDeque::new(),
            stream_fallback: None,
        }
    }
}

impl MockLanguageModelBuilder {
    /// Sets the provider id.
    #[must_use]
    pub fn provider(mut self, provider: impl Into<ProviderId>) -> Self {
        self.provider = provider.into();
        self
    }

    /// Sets the model id.
    #[must_use]
    pub fn model_id(mut self, model_id: impl Into<ModelId>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Sets the supported URL patterns.
    #[must_use]
    pub fn supported_urls(mut self, supported_urls: SupportedUrls) -> Self {
        self.supported_urls = supported_urls;
        self
    }

    /// Queues one generate response.
    #[must_use]
    pub fn generate(mut self, result: GenerateResult) -> Self {
        self.generate_queue.push_back(Ok(result));
        self
    }

    /// Queues one failing generate call.
    #[must_use]
    pub fn generate_error(mut self, error: ProviderError) -> Self {
        self.generate_queue.push_back(Err(error));
        self
    }

    /// Answers generate calls beyond the queue with a clone of `result`.
    #[must_use]
    pub fn generate_repeat(mut self, result: GenerateResult) -> Self {
        self.generate_fallback = Some(Arc::new(move |_| {
            Box::pin(std::future::ready(Ok(result.clone())))
        }));
        self
    }

    /// Answers generate calls beyond the queue by calling `function`.
    #[must_use]
    pub fn generate_with<F, Fut>(mut self, function: F) -> Self
    where
        F: Fn(CallOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<GenerateResult, ProviderError>> + Send + 'static,
    {
        self.generate_fallback = Some(Arc::new(move |options| Box::pin(function(options))));
        self
    }

    /// Queues one stream response made of `parts`.
    #[must_use]
    pub fn stream(mut self, parts: Vec<StreamPart>) -> Self {
        self.stream_queue.push_back(Ok(parts));
        self
    }

    /// Queues one failing stream call (the failure happens before the first
    /// part).
    #[must_use]
    pub fn stream_error(mut self, error: ProviderError) -> Self {
        self.stream_queue.push_back(Err(error));
        self
    }

    /// Answers stream calls beyond the queue with a clone of `parts`.
    #[must_use]
    pub fn stream_repeat(mut self, parts: Vec<StreamPart>) -> Self {
        self.stream_fallback = Some(Arc::new(move |_| {
            Box::pin(std::future::ready(Ok(simulate_stream(parts.clone()))))
        }));
        self
    }

    /// Answers stream calls beyond the queue by calling `function`.
    #[must_use]
    pub fn stream_with<F, Fut>(mut self, function: F) -> Self
    where
        F: Fn(CallOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<StreamResult, ProviderError>> + Send + 'static,
    {
        self.stream_fallback = Some(Arc::new(move |options| Box::pin(function(options))));
        self
    }

    /// Builds the mock.
    #[must_use]
    pub fn build(self) -> MockLanguageModel {
        MockLanguageModel {
            provider: self.provider,
            model_id: self.model_id,
            supported_urls: self.supported_urls,
            generate_queue: Mutex::new(self.generate_queue),
            generate_fallback: self.generate_fallback,
            stream_queue: Mutex::new(self.stream_queue),
            stream_fallback: self.stream_fallback,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Builds the mock behind an [`Arc`], for sharing between the call and
    /// the assertions.
    #[must_use]
    pub fn build_shared(self) -> Arc<MockLanguageModel> {
        Arc::new(self.build())
    }
}

/// An API call error with `status` against a placeholder URL. The retry
/// classification follows the status code.
#[must_use]
pub fn api_call_error(status: StatusCode, message: impl Into<String>) -> ProviderError {
    let url = match Url::parse("https://mock.invalid/v1/call") {
        Ok(url) => url,
        Err(_) => unreachable!("the placeholder URL is valid"),
    };
    ProviderError::ApiCall(Box::new(
        ApiCallError::new(message, url).with_status(status),
    ))
}
