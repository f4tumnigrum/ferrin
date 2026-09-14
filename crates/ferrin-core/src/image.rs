//! Image generation: [`generate_image`] splits `n` images into provider
//! calls, runs them concurrently and retries empty results.
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §2.

use std::future::IntoFuture;
use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_provider_util::media_type::detect_media_type_for;
use ferrin_spec::AspectRatio;
use ferrin_spec::BoxFuture;
use ferrin_spec::DynImageModel;
use ferrin_spec::ImageModelRef;
use ferrin_spec::ImageSize;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use ferrin_spec::error::NoContentGeneratedError;
use ferrin_spec::error::ProviderError;
pub use ferrin_spec::image_model::ImageFile;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageResult;
pub use ferrin_spec::image_model::ImageUsage;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::add_optional;
use crate::modality::impl_modality_builder;
use crate::modality::merge_provider_metadata;
use crate::registry::ProviderRegistry;
use crate::registry::default::resolve_model;
use crate::retry::RetryPolicy;
use crate::retry::retry_with;
use crate::telemetry::ModelIdentity;
use crate::telemetry::spans;

/// Marker message of the internal retryable "empty result" error.
const NO_IMAGE_MESSAGE: &str = "no image generated";

/// Media type used when neither the provider nor detection knows better.
const DEFAULT_IMAGE_MEDIA_TYPE: &str = "image/png";

/// A generated image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedImage {
    /// Image bytes.
    pub data: Bytes,
    /// Media type (reported, detected, or `image/png`).
    pub media_type: MediaType,
    /// Provider metadata of this image (the entry of the provider's
    /// `images` list at this index).
    pub provider_metadata: Option<ProviderMetadata>,
}

impl GeneratedImage {
    /// The image bytes as base64.
    #[must_use]
    pub fn base64(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }
}

/// One provider call of [`generate_image`].
#[derive(Debug, Clone, PartialEq)]
pub struct ImageCall {
    /// Images of this call.
    pub images: Vec<GeneratedImage>,
    /// Warnings of this call.
    pub warnings: Vec<Warning>,
    /// Response metadata of this call.
    pub response: ResponseMetadata,
    /// Provider metadata of this call.
    pub provider_metadata: Option<ProviderMetadata>,
    /// Usage of this call.
    pub usage: Option<ImageUsage>,
}

/// Result of [`generate_image`].
#[derive(Debug, Clone, PartialEq)]
pub struct GenerateImageResult {
    /// All images, in call order.
    pub images: Vec<GeneratedImage>,
    /// The individual provider calls.
    pub calls: Vec<ImageCall>,
    /// Warnings of all calls.
    pub warnings: Vec<Warning>,
    /// Response metadata of all calls.
    pub responses: Vec<ResponseMetadata>,
    /// Provider metadata merged over all calls.
    pub provider_metadata: ProviderMetadata,
    /// Usage summed over all calls.
    pub usage: ImageUsage,
}

impl GenerateImageResult {
    /// The first image.
    #[must_use]
    pub fn image(&self) -> Option<&GeneratedImage> {
        self.images.first()
    }
}

/// Generates images from a text prompt.
#[must_use]
pub fn generate_image(model: impl Into<ImageModelRef>, prompt: impl Into<String>) -> GenerateImage {
    GenerateImage {
        model: model.into(),
        prompt: Some(prompt.into()),
        n: 1,
        max_images_per_call: None,
        size: None,
        aspect_ratio: None,
        seed: None,
        files: Vec::new(),
        mask: None,
        base: ModalityOptions::default(),
    }
}

/// Generates images from reference images (image-to-image), optionally
/// guided by a prompt set with [`GenerateImage::prompt`].
#[must_use]
pub fn edit_image(model: impl Into<ImageModelRef>, files: Vec<ImageFile>) -> GenerateImage {
    GenerateImage {
        model: model.into(),
        prompt: None,
        n: 1,
        max_images_per_call: None,
        size: None,
        aspect_ratio: None,
        seed: None,
        files,
        mask: None,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`generate_image`]; `.await` runs the calls.
#[derive(Debug)]
pub struct GenerateImage {
    model: ImageModelRef,
    prompt: Option<String>,
    n: u32,
    max_images_per_call: Option<u32>,
    size: Option<ImageSize>,
    aspect_ratio: Option<AspectRatio>,
    seed: Option<u64>,
    files: Vec<ImageFile>,
    mask: Option<ImageFile>,
    base: ModalityOptions,
}

impl GenerateImage {
    /// Sets the text prompt.
    #[must_use]
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Number of images to generate (default 1).
    #[must_use]
    pub fn n(mut self, n: u32) -> Self {
        self.n = n;
        self
    }

    /// Overrides the model's images-per-call limit.
    #[must_use]
    pub fn max_images_per_call(mut self, max_images_per_call: u32) -> Self {
        self.max_images_per_call = Some(max_images_per_call);
        self
    }

    /// Image size (`1024x1024`).
    #[must_use]
    pub fn size(mut self, size: ImageSize) -> Self {
        self.size = Some(size);
        self
    }

    /// Aspect ratio (`16:9`).
    #[must_use]
    pub fn aspect_ratio(mut self, aspect_ratio: AspectRatio) -> Self {
        self.aspect_ratio = Some(aspect_ratio);
        self
    }

    /// Seed for reproducible generation.
    #[must_use]
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Reference images.
    #[must_use]
    pub fn files(mut self, files: Vec<ImageFile>) -> Self {
        self.files = files;
        self
    }

    /// Adds one reference image.
    #[must_use]
    pub fn file(mut self, file: ImageFile) -> Self {
        self.files.push(file);
        self
    }

    /// Mask for inpainting.
    #[must_use]
    pub fn mask(mut self, mask: ImageFile) -> Self {
        self.mask = Some(mask);
        self
    }
}

impl_modality_builder!(GenerateImage);

impl IntoFuture for GenerateImage {
    type Output = Result<GenerateImageResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(run(self))
    }
}

/// Provider metadata of one image: the `images[index]` entry of every
/// provider that lists per-image metadata.
pub(crate) fn image_provider_metadata(
    provider_metadata: Option<&ProviderMetadata>,
    index: usize,
) -> Option<ProviderMetadata> {
    let mut result: Option<ProviderMetadata> = None;
    for (provider, metadata) in provider_metadata? {
        if let Some(JsonValue::Array(images)) = metadata.get("images")
            && let Some(JsonValue::Object(entry)) = images.get(index)
        {
            result
                .get_or_insert_with(ProviderMetadata::new)
                .insert(provider.clone(), entry.clone());
        }
    }
    result
}

/// Converts the images of one provider result.
pub(crate) fn convert_images(result: &ImageResult) -> Vec<GeneratedImage> {
    result
        .images
        .iter()
        .enumerate()
        .map(|(index, image)| GeneratedImage {
            data: image.data.clone(),
            media_type: image
                .media_type
                .clone()
                .or_else(|| detect_media_type_for(&image.data, "image"))
                .unwrap_or_else(|| MediaType::new(DEFAULT_IMAGE_MEDIA_TYPE)),
            provider_metadata: image_provider_metadata(result.provider_metadata.as_ref(), index),
        })
        .collect()
}

/// Adds two optional usages field by field.
pub(crate) fn add_image_usage(total: ImageUsage, usage: &ImageUsage) -> ImageUsage {
    ImageUsage {
        input_tokens: add_optional(total.input_tokens, usage.input_tokens),
        output_tokens: add_optional(total.output_tokens, usage.output_tokens),
        total_tokens: add_optional(total.total_tokens, usage.total_tokens),
    }
}

fn no_image_error() -> ProviderError {
    ProviderError::NoContentGenerated(NoContentGeneratedError::with_message(NO_IMAGE_MESSAGE))
}

fn is_no_image(error: &ProviderError) -> bool {
    matches!(error, ProviderError::NoContentGenerated(inner) if inner.message == NO_IMAGE_MESSAGE)
}

fn ended_without_image(error: &Error) -> bool {
    match error {
        Error::Provider(error) => is_no_image(error),
        Error::Retry { errors, .. } => errors.last().is_some_and(is_no_image),
        _ => false,
    }
}

/// One provider call (owned so that it can run on a task).
struct ImageCallTask {
    model: Arc<dyn DynImageModel>,
    options: ImageOptions,
    retry_policy: RetryPolicy,
    cancellation: CancellationToken,
}

impl ImageCallTask {
    /// Runs the call with retries; returns every provider result received
    /// (empty results are retried and kept for their metadata).
    async fn run(self) -> Result<Vec<ImageResult>, Error> {
        let results: Arc<Mutex<Vec<ImageResult>>> = Arc::new(Mutex::new(Vec::new()));
        let outcome = retry_with(
            &self.retry_policy,
            &self.cancellation,
            |error| error.is_retryable() || is_no_image(error),
            |_| {
                let results = Arc::clone(&results);
                let mut options = self.options.clone();
                options.cancellation = self.cancellation.child_token();
                let model = Arc::clone(&self.model);
                async move {
                    let result = model.do_generate(options).await.map_err(Error::from)?;
                    let empty = result.images.is_empty() && result.is_retryable != Some(false);
                    results
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(result);
                    if empty {
                        Err(Error::from(no_image_error()))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .await;
        match outcome {
            Ok(()) => {}
            Err(error) if ended_without_image(&error) => {}
            Err(error) => return Err(error),
        }
        let collected = std::mem::take(
            &mut *results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        Ok(collected)
    }
}

async fn run(builder: GenerateImage) -> Result<GenerateImageResult, Error> {
    if builder.n == 0 {
        return Err(Error::invalid_argument("n", "must be at least 1"));
    }
    if builder.max_images_per_call == Some(0) {
        return Err(Error::invalid_argument(
            "max_images_per_call",
            "must be at least 1",
        ));
    }
    let model = resolve_model(&builder.model, ProviderRegistry::image_model)?;
    let identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
    let span = spans::modality_span("image", &identity);
    let base = builder.base.clone();
    base.run(|_, token| run_calls(model, identity, builder, token).instrument(span))
        .await
}

async fn run_calls(
    model: Arc<dyn DynImageModel>,
    identity: ModelIdentity,
    builder: GenerateImage,
    cancellation: CancellationToken,
) -> Result<GenerateImageResult, Error> {
    let per_call = builder
        .max_images_per_call
        .map(|limit| usize::try_from(limit).unwrap_or(usize::MAX))
        .or_else(|| model.max_images_per_call().filter(|limit| *limit > 0))
        .unwrap_or(1);
    let n = usize::try_from(builder.n).unwrap_or(usize::MAX);
    let call_count = n.div_ceil(per_call);
    let counts: Vec<u32> = (0..call_count)
        .map(|index| {
            let remaining = n.saturating_sub(index.saturating_mul(per_call));
            u32::try_from(remaining.min(per_call)).unwrap_or(u32::MAX)
        })
        .collect();

    let template = ImageOptions {
        prompt: builder.prompt.clone(),
        n: 1,
        size: builder.size,
        aspect_ratio: builder.aspect_ratio,
        seed: builder.seed,
        files: builder.files.clone(),
        mask: builder.mask.clone(),
        provider_options: builder.base.provider_options.clone(),
        headers: builder.base.request_headers(),
        cancellation: cancellation.clone(),
    };
    let make_task = |count: u32| ImageCallTask {
        model: Arc::clone(&model),
        options: ImageOptions {
            n: count,
            ..template.clone()
        },
        retry_policy: builder.base.retry_policy.clone(),
        cancellation: cancellation.clone(),
    };

    let mut groups: Vec<Option<Vec<ImageResult>>> = (0..counts.len()).map(|_| None).collect();
    if let [count] = counts.as_slice() {
        groups = vec![Some(make_task(*count).run().await?)];
    } else {
        let mut tasks: JoinSet<(usize, Result<Vec<ImageResult>, Error>)> = JoinSet::new();
        for (index, count) in counts.iter().enumerate() {
            let task = make_task(*count);
            tasks.spawn(async move { (index, task.run().await) });
        }
        while let Some(joined) = tasks.join_next().await {
            let (index, result) =
                joined.map_err(|error| Error::message(format!("image task failed: {error}")))?;
            if let Some(slot) = groups.get_mut(index) {
                *slot = Some(result?);
            }
        }
    }

    let mut images: Vec<GeneratedImage> = Vec::new();
    let mut calls: Vec<ImageCall> = Vec::new();
    let mut warnings: Vec<Warning> = Vec::new();
    let mut responses: Vec<ResponseMetadata> = Vec::new();
    let mut provider_metadata = ProviderMetadata::new();
    let mut usage = ImageUsage::default();
    for result in groups.into_iter().flatten().flatten() {
        let call_images = convert_images(&result);
        images.extend(call_images.iter().cloned());
        warnings.extend(result.warnings.iter().cloned());
        responses.push(result.response.clone());
        if let Some(call_usage) = &result.usage {
            usage = add_image_usage(usage, call_usage);
        }
        if let Some(metadata) = &result.provider_metadata {
            merge_provider_metadata(&mut provider_metadata, metadata);
        }
        calls.push(ImageCall {
            images: call_images,
            warnings: result.warnings,
            response: result.response,
            provider_metadata: result.provider_metadata,
            usage: result.usage,
        });
    }
    if images.is_empty() {
        return Err(Error::NoImageGenerated { responses });
    }
    spans::log_warnings(&warnings, &identity);
    Ok(GenerateImageResult {
        images,
        calls,
        warnings,
        responses,
        provider_metadata,
        usage,
    })
}
