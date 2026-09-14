//! Mock models for the modality tests.

use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use bytes::Bytes;
use ferrin_core::RetryPolicy;
use ferrin_spec::EmbeddingModel;
use ferrin_spec::ImageModel;
use ferrin_spec::MediaType;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::embedding_model::EmbedResult;
use ferrin_spec::embedding_model::EmbeddingUsage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::image_model::GeneratedImage as ModelImage;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageResult;
use ferrin_spec::image_model::ImageUsage;

/// PNG magic bytes followed by padding.
pub(crate) const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n0000";

/// A retry policy without delays.
pub(crate) fn fast_retry(max_retries: u32) -> RetryPolicy {
    RetryPolicy {
        max_retries,
        initial_delay: std::time::Duration::ZERO,
        ..RetryPolicy::default()
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Embedding mock: each value becomes `[len, 1.0]`, usage counts values.
pub(crate) struct EmbedMock {
    provider: ProviderId,
    model_id: ModelId,
    pub(crate) max_per_call: Option<usize>,
    pub(crate) max_bytes: Option<usize>,
    pub(crate) parallel: bool,
    /// Number of embeddings to drop from the first response (to simulate a
    /// provider returning too few vectors).
    pub(crate) drop_first: usize,
    /// Fails the first `fail_first` calls with a retryable error.
    pub(crate) fail_first: usize,
    pub(crate) calls: Mutex<Vec<Vec<String>>>,
    failures: AtomicUsize,
}

impl EmbedMock {
    pub(crate) fn new() -> Self {
        Self {
            provider: ProviderId::new("mock"),
            model_id: ModelId::new("embed-mock"),
            max_per_call: None,
            max_bytes: None,
            parallel: true,
            drop_first: 0,
            fail_first: 0,
            calls: Mutex::new(Vec::new()),
            failures: AtomicUsize::new(0),
        }
    }

    pub(crate) fn calls(&self) -> Vec<Vec<String>> {
        lock(&self.calls).clone()
    }
}

impl EmbeddingModel for EmbedMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_embeddings_per_call(&self) -> Option<usize> {
        self.max_per_call
    }

    fn max_input_bytes_per_call(&self) -> Option<usize> {
        self.max_bytes
    }

    fn supports_parallel_calls(&self) -> bool {
        self.parallel
    }

    fn do_embed(
        &self,
        options: EmbedOptions,
    ) -> impl Future<Output = Result<EmbedResult, ProviderError>> + Send {
        let call_number = {
            let mut calls = lock(&self.calls);
            calls.push(options.values.clone());
            calls.len()
        };
        let fail = self.failures.load(Ordering::SeqCst) < self.fail_first;
        if fail {
            self.failures.fetch_add(1, Ordering::SeqCst);
        }
        let drop = if call_number == 1 { self.drop_first } else { 0 };
        async move {
            if fail {
                return Err(ferrin_testing::api_call_error(
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    "overloaded",
                ));
            }
            let count = options.values.len();
            #[allow(clippy::cast_precision_loss, reason = "test values are tiny")]
            let mut embeddings: Vec<Vec<f32>> = options
                .values
                .iter()
                .map(|value| vec![value.len() as f32, 1.0])
                .collect();
            embeddings.truncate(count.saturating_sub(drop));
            Ok(EmbedResult {
                embeddings,
                usage: Some(EmbeddingUsage {
                    tokens: count as u64,
                }),
                provider_metadata: None,
                response: ResponseMetadata {
                    model_id: Some(ModelId::new("embed-mock")),
                    ..ResponseMetadata::default()
                },
                warnings: Vec::new(),
            })
        }
    }
}

/// Image mock: the first `empty_first` calls return no images.
pub(crate) struct ImageMock {
    provider: ProviderId,
    model_id: ModelId,
    pub(crate) max_per_call: Option<usize>,
    pub(crate) empty_first: usize,
    pub(crate) empty_not_retryable: bool,
    pub(crate) calls: Mutex<Vec<ImageOptions>>,
}

impl ImageMock {
    pub(crate) fn new() -> Self {
        Self {
            provider: ProviderId::new("mock"),
            model_id: ModelId::new("image-mock"),
            max_per_call: None,
            empty_first: 0,
            empty_not_retryable: false,
            calls: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn call_counts(&self) -> Vec<u32> {
        lock(&self.calls).iter().map(|options| options.n).collect()
    }
}

impl ImageModel for ImageMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Option<usize> {
        self.max_per_call
    }

    fn do_generate(
        &self,
        options: ImageOptions,
    ) -> impl Future<Output = Result<ImageResult, ProviderError>> + Send {
        let call_number = {
            let mut calls = lock(&self.calls);
            calls.push(options.clone());
            calls.len()
        };
        let empty = call_number <= self.empty_first;
        let not_retryable = self.empty_not_retryable;
        async move {
            let images = if empty {
                Vec::new()
            } else {
                (0..options.n)
                    .map(|index| ModelImage {
                        data: Bytes::from([PNG_BYTES, &[index as u8]].concat()),
                        media_type: None,
                    })
                    .collect()
            };
            Ok(ImageResult {
                images,
                is_retryable: (empty && not_retryable).then_some(false),
                warnings: Vec::new(),
                provider_metadata: None,
                response: ResponseMetadata {
                    model_id: Some(ModelId::new("image-mock")),
                    ..ResponseMetadata::default()
                },
                usage: Some(ImageUsage {
                    input_tokens: Some(1),
                    output_tokens: Some(u64::from(options.n)),
                    total_tokens: None,
                }),
            })
        }
    }
}

pub(crate) fn media(text: &str) -> MediaType {
    MediaType::new(text)
}
