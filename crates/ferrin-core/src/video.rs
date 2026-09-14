//! Video generation (feature `video`): [`generate_video`] supports the
//! synchronous flow (`do_generate`) and the asynchronous flow
//! (`do_start`/`do_status`, with polling or a webhook).
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §6.

use std::fmt;
use std::future::IntoFuture;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use ferrin_provider_util::ids::generate_id;
use ferrin_provider_util::media_type::detect_media_type_for;
use ferrin_spec::BoxFuture;
use ferrin_spec::DynVideoModel;
use ferrin_spec::FileData;
use ferrin_spec::ImageSize;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::VideoModelRef;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
pub use ferrin_spec::video_model::FrameImage;
pub use ferrin_spec::video_model::FrameType;
pub use ferrin_spec::video_model::VideoAspectRatio;
pub use ferrin_spec::video_model::VideoFile;
use ferrin_spec::video_model::VideoOptions;
use ferrin_spec::video_model::VideoResult;
use ferrin_spec::video_model::VideoStartOptions;
use ferrin_spec::video_model::VideoStatusOptions;
use ferrin_spec::video_model::VideoStatusResult;
pub use ferrin_spec::video_model::WebhookFactory;
pub use ferrin_spec::video_model::WebhookHandle;
pub use ferrin_spec::video_model::WebhookPayload;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
use crate::modality::merge_provider_metadata;
use crate::prompt::DefaultDownloader;
use crate::prompt::DownloadFn;
use crate::prompt::DownloadRequest;
use crate::registry::ProviderRegistry;
use crate::registry::default::resolve_model;
use crate::retry::RetryPolicy;
use crate::retry::retry;
use crate::telemetry::ModelIdentity;
use crate::telemetry::spans;
use crate::timeout::TimeoutScope;

/// Media type used when nothing better is known.
const DEFAULT_VIDEO_MEDIA_TYPE: &str = "video/mp4";

/// Media type treated as unknown.
const GENERIC_MEDIA_TYPE: &str = "application/octet-stream";

/// Header carrying the idempotency key of `do_start`.
const IDEMPOTENCY_KEY: &str = "idempotency-key";

/// Polling configuration of the asynchronous flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollConfig {
    /// Delay between status requests (default 5 s).
    pub interval: Duration,
    /// Maximum time to wait for completion, polling or via webhook
    /// (default 10 min).
    pub timeout: Duration,
    /// Maximum number of status requests (default unlimited).
    pub max_attempts: Option<u32>,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(600),
            max_attempts: None,
        }
    }
}

/// A generated video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedVideo {
    /// Video bytes.
    pub data: Bytes,
    /// Media type (reported, downloaded, detected, or `video/mp4`).
    pub media_type: MediaType,
}

/// Result of [`generate_video`].
#[derive(Debug, Clone, PartialEq)]
pub struct GenerateVideoResult {
    /// All videos, in call order.
    pub videos: Vec<GeneratedVideo>,
    /// Warnings of all calls.
    pub warnings: Vec<Warning>,
    /// Response metadata of all calls.
    pub responses: Vec<ResponseMetadata>,
    /// Provider metadata merged over all calls.
    pub provider_metadata: ProviderMetadata,
}

impl GenerateVideoResult {
    /// The first video.
    #[must_use]
    pub fn video(&self) -> Option<&GeneratedVideo> {
        self.videos.first()
    }
}

/// Generates videos from a text prompt.
#[must_use]
pub fn generate_video(model: impl Into<VideoModelRef>, prompt: impl Into<String>) -> GenerateVideo {
    GenerateVideo {
        model: model.into(),
        prompt: Some(prompt.into()),
        n: 1,
        max_videos_per_call: None,
        aspect_ratio: None,
        resolution: None,
        duration: None,
        fps: None,
        seed: None,
        image: None,
        frame_images: Vec::new(),
        input_references: Vec::new(),
        generate_audio: None,
        poll: None,
        webhook: None,
        download: None,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`generate_video`]; `.await` runs the calls.
pub struct GenerateVideo {
    model: VideoModelRef,
    prompt: Option<String>,
    n: u32,
    max_videos_per_call: Option<u32>,
    aspect_ratio: Option<VideoAspectRatio>,
    resolution: Option<ImageSize>,
    duration: Option<f64>,
    fps: Option<u32>,
    seed: Option<u64>,
    image: Option<VideoFile>,
    frame_images: Vec<FrameImage>,
    input_references: Vec<VideoFile>,
    generate_audio: Option<bool>,
    poll: Option<PollConfig>,
    webhook: Option<WebhookFactory>,
    download: Option<Arc<dyn DownloadFn>>,
    base: ModalityOptions,
}

impl fmt::Debug for GenerateVideo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenerateVideo")
            .field("model", &self.model)
            .field("prompt", &self.prompt)
            .field("n", &self.n)
            .field("max_videos_per_call", &self.max_videos_per_call)
            .field("aspect_ratio", &self.aspect_ratio)
            .field("resolution", &self.resolution)
            .field("duration", &self.duration)
            .field("fps", &self.fps)
            .field("seed", &self.seed)
            .field("image", &self.image)
            .field("frame_images", &self.frame_images)
            .field("input_references", &self.input_references)
            .field("generate_audio", &self.generate_audio)
            .field("poll", &self.poll)
            .field("has_webhook", &self.webhook.is_some())
            .field("has_download", &self.download.is_some())
            .field("base", &self.base)
            .finish()
    }
}

impl GenerateVideo {
    /// Sets the text prompt.
    #[must_use]
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Number of videos to generate (default 1).
    #[must_use]
    pub fn n(mut self, n: u32) -> Self {
        self.n = n;
        self
    }

    /// Overrides the model's videos-per-call limit.
    #[must_use]
    pub fn max_videos_per_call(mut self, max_videos_per_call: u32) -> Self {
        self.max_videos_per_call = Some(max_videos_per_call);
        self
    }

    /// Aspect ratio.
    #[must_use]
    pub fn aspect_ratio(mut self, aspect_ratio: VideoAspectRatio) -> Self {
        self.aspect_ratio = Some(aspect_ratio);
        self
    }

    /// Resolution (`1280x720`).
    #[must_use]
    pub fn resolution(mut self, resolution: ImageSize) -> Self {
        self.resolution = Some(resolution);
        self
    }

    /// Duration in seconds.
    #[must_use]
    pub fn duration(mut self, seconds: f64) -> Self {
        self.duration = Some(seconds);
        self
    }

    /// Frames per second.
    #[must_use]
    pub fn fps(mut self, fps: u32) -> Self {
        self.fps = Some(fps);
        self
    }

    /// Seed for reproducible generation.
    #[must_use]
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Source image (image-to-video).
    #[must_use]
    pub fn image(mut self, image: VideoFile) -> Self {
        self.image = Some(image);
        self
    }

    /// First/last frame images.
    #[must_use]
    pub fn frame_images(mut self, frame_images: Vec<FrameImage>) -> Self {
        self.frame_images = frame_images;
        self
    }

    /// Reference inputs.
    #[must_use]
    pub fn input_references(mut self, input_references: Vec<VideoFile>) -> Self {
        self.input_references = input_references;
        self
    }

    /// Whether to generate audio.
    #[must_use]
    pub fn generate_audio(mut self, generate_audio: bool) -> Self {
        self.generate_audio = Some(generate_audio);
        self
    }

    /// Uses the asynchronous flow with polling.
    #[must_use]
    pub fn poll(mut self, poll: PollConfig) -> Self {
        self.poll = Some(poll);
        self
    }

    /// Uses the asynchronous flow with a webhook created by `factory`
    /// (falls back to polling when the model does not support webhooks).
    #[must_use]
    pub fn webhook(mut self, factory: WebhookFactory) -> Self {
        self.webhook = Some(factory);
        self
    }

    /// Sets the function used to fetch videos returned as URLs.
    #[must_use]
    pub fn download(mut self, download: Arc<dyn DownloadFn>) -> Self {
        self.download = Some(download);
        self
    }
}

impl_modality_builder!(GenerateVideo);

impl IntoFuture for GenerateVideo {
    type Output = Result<GenerateVideoResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(run(self))
    }
}

/// One provider call (owned so that it can run on a task).
struct VideoCallTask {
    model: Arc<dyn DynVideoModel>,
    options: VideoOptions,
    retry_policy: RetryPolicy,
    cancellation: CancellationToken,
    use_operations: bool,
    poll: PollConfig,
    webhook: Option<WebhookFactory>,
}

impl VideoCallTask {
    async fn run(self) -> Result<VideoResult, Error> {
        if self.use_operations {
            self.run_operations().await
        } else {
            let model = &self.model;
            let options = &self.options;
            let token = &self.cancellation;
            retry(&self.retry_policy, &self.cancellation, |_| {
                let mut options = options.clone();
                options.cancellation = token.child_token();
                async move { model.do_generate(options).await.map_err(Error::from) }
            })
            .await
        }
    }

    async fn run_operations(self) -> Result<VideoResult, Error> {
        let mut warnings: Vec<Warning> = Vec::new();
        let mut webhook_url = None;
        let mut received = None;
        if let Some(factory) = self.webhook.clone() {
            if self.model.supports_webhook() {
                let handle = self
                    .model
                    .handle_webhook(factory)
                    .await
                    .map_err(Error::from)?;
                webhook_url = Some(handle.url);
                received = Some(handle.received);
            } else {
                warnings.push(Warning::unsupported_with_details(
                    "webhook",
                    "This model does not support webhooks. Falling back to polling.",
                ));
            }
        }

        // `do_start` is billable: one idempotency key per logical start,
        // minted outside the retry loop; a caller-supplied key wins.
        let mut start_options = self.options.clone();
        if !start_options.headers.contains(IDEMPOTENCY_KEY) {
            start_options.headers = start_options
                .headers
                .with(IDEMPOTENCY_KEY, &format!("ferrin_vid_{}", generate_id()));
        }
        let model = &self.model;
        let token = &self.cancellation;
        let start = retry(&self.retry_policy, &self.cancellation, |_| {
            let mut options = start_options.clone();
            options.cancellation = token.child_token();
            let webhook_url = webhook_url.clone();
            async move {
                model
                    .do_start(VideoStartOptions {
                        options,
                        webhook_url,
                    })
                    .await
                    .map_err(Error::from)
            }
        })
        .await?;
        warnings.extend(start.warnings);
        let mut provider_metadata = start.provider_metadata;
        let started = Instant::now();
        let deadline = started + self.poll.timeout;

        if let Some(received) = received {
            let wait = tokio::time::timeout_at(deadline, received);
            match wait.await {
                Ok(Ok(_payload)) => {}
                Ok(Err(error)) => return Err(Error::from(error)),
                Err(_) => {
                    return Err(Error::Timeout {
                        scope: TimeoutScope::Total,
                        elapsed: started.elapsed(),
                    });
                }
            }
        }
        let waits_for_webhook = webhook_url.is_some();

        let mut attempts: u32 = 0;
        loop {
            if !waits_for_webhook {
                if Instant::now() >= deadline {
                    return Err(Error::Timeout {
                        scope: TimeoutScope::Total,
                        elapsed: started.elapsed(),
                    });
                }
                if let Some(max_attempts) = self.poll.max_attempts
                    && attempts >= max_attempts
                {
                    return Err(Error::message(format!(
                        "video generation did not complete after {max_attempts} status requests"
                    )));
                }
                let sleep =
                    tokio::time::sleep_until((Instant::now() + self.poll.interval).min(deadline));
                tokio::select! {
                    () = sleep => {}
                    () = self.cancellation.cancelled() => return Err(Error::Cancelled),
                }
                if Instant::now() >= deadline {
                    return Err(Error::Timeout {
                        scope: TimeoutScope::Total,
                        elapsed: started.elapsed(),
                    });
                }
            }
            attempts = attempts.saturating_add(1);
            let operation = start.operation.clone();
            let headers = self.options.headers.clone();
            let status = retry(&self.retry_policy, &self.cancellation, |_| {
                let options = VideoStatusOptions {
                    operation: operation.clone(),
                    headers: headers.clone(),
                    cancellation: token.child_token(),
                };
                async move { model.do_status(options).await.map_err(Error::from) }
            })
            .await?;
            match status {
                VideoStatusResult::Error { error, .. } => {
                    return Err(Error::message(format!("video generation failed: {error}")));
                }
                VideoStatusResult::Pending {
                    warnings: status_warnings,
                    provider_metadata: status_metadata,
                    ..
                } => {
                    warnings.extend(status_warnings);
                    if let Some(metadata) = status_metadata {
                        merge_provider_metadata(
                            provider_metadata.get_or_insert_with(ProviderMetadata::new),
                            &metadata,
                        );
                    }
                    if waits_for_webhook {
                        return Err(Error::message(
                            "video generation did not complete after the webhook notification",
                        ));
                    }
                }
                VideoStatusResult::Completed {
                    videos,
                    warnings: status_warnings,
                    provider_metadata: status_metadata,
                    response,
                } => {
                    warnings.extend(status_warnings);
                    if let Some(metadata) = status_metadata {
                        merge_provider_metadata(
                            provider_metadata.get_or_insert_with(ProviderMetadata::new),
                            &metadata,
                        );
                    }
                    return Ok(VideoResult {
                        videos,
                        warnings,
                        provider_metadata,
                        response,
                    });
                }
                #[allow(unreachable_patterns, reason = "VideoStatusResult is non-exhaustive")]
                _ => {
                    return Err(Error::message("unknown video status"));
                }
            }
        }
    }
}

fn usable_media_type(media_type: &MediaType) -> bool {
    !media_type.as_str().is_empty() && media_type.as_str() != GENERIC_MEDIA_TYPE
}

async fn run(builder: GenerateVideo) -> Result<GenerateVideoResult, Error> {
    if builder.n == 0 {
        return Err(Error::invalid_argument("n", "must be at least 1"));
    }
    if builder.max_videos_per_call == Some(0) {
        return Err(Error::invalid_argument(
            "max_videos_per_call",
            "must be at least 1",
        ));
    }
    let model = resolve_model(&builder.model, ProviderRegistry::video_model)?;
    let identity = ModelIdentity::new(model.provider().clone(), model.model_id().clone());
    let span = spans::modality_span("video", &identity);
    let base = builder.base.clone();
    base.run(|_, token| run_calls(model, identity, builder, token).instrument(span))
        .await
}

async fn run_calls(
    model: Arc<dyn DynVideoModel>,
    identity: ModelIdentity,
    builder: GenerateVideo,
    cancellation: CancellationToken,
) -> Result<GenerateVideoResult, Error> {
    let supports_generate = model.supports_generate();
    let supports_operations = model.supports_operations();
    let wants_operations = builder.poll.is_some() || builder.webhook.is_some();
    if !supports_generate && !supports_operations {
        return Err(Error::from(ProviderError::unsupported(format!(
            "video generation (model `{}` implements neither synchronous nor asynchronous generation)",
            identity.model_id
        ))));
    }
    let use_operations = supports_operations && (wants_operations || !supports_generate);
    if wants_operations && !supports_operations {
        spans::log_warnings(
            &[Warning::other(
                "poll/webhook options were provided but the model does not support \
                 asynchronous operations; falling back to synchronous generation",
            )],
            &identity,
        );
    }

    let per_call = builder
        .max_videos_per_call
        .map(|limit| usize::try_from(limit).unwrap_or(usize::MAX))
        .or_else(|| model.max_videos_per_call().filter(|limit| *limit > 0))
        .unwrap_or(1);
    let n = usize::try_from(builder.n).unwrap_or(usize::MAX);
    let counts: Vec<u32> = (0..n.div_ceil(per_call))
        .map(|index| {
            let remaining = n.saturating_sub(index.saturating_mul(per_call));
            u32::try_from(remaining.min(per_call)).unwrap_or(u32::MAX)
        })
        .collect();

    let template = VideoOptions {
        prompt: builder.prompt.clone(),
        n: 1,
        aspect_ratio: builder.aspect_ratio,
        resolution: builder.resolution,
        duration: builder.duration,
        fps: builder.fps,
        seed: builder.seed,
        image: builder.image.clone(),
        frame_images: builder.frame_images.clone(),
        input_references: builder.input_references.clone(),
        generate_audio: builder.generate_audio,
        provider_options: builder.base.provider_options.clone(),
        headers: builder.base.request_headers(),
        cancellation: cancellation.clone(),
    };
    let poll = builder.poll.clone().unwrap_or_default();
    let make_task = |count: u32| VideoCallTask {
        model: Arc::clone(&model),
        options: VideoOptions {
            n: count,
            ..template.clone()
        },
        retry_policy: builder.base.retry_policy.clone(),
        cancellation: cancellation.clone(),
        use_operations,
        poll: poll.clone(),
        webhook: builder.webhook.clone(),
    };

    let mut results: Vec<Option<VideoResult>> = (0..counts.len()).map(|_| None).collect();
    if let [count] = counts.as_slice() {
        results = vec![Some(make_task(*count).run().await?)];
    } else {
        let mut tasks: JoinSet<(usize, Result<VideoResult, Error>)> = JoinSet::new();
        for (index, count) in counts.iter().enumerate() {
            let task = make_task(*count);
            tasks.spawn(async move { (index, task.run().await) });
        }
        while let Some(joined) = tasks.join_next().await {
            let (index, result) =
                joined.map_err(|error| Error::message(format!("video task failed: {error}")))?;
            if let Some(slot) = results.get_mut(index) {
                *slot = Some(result?);
            }
        }
    }

    let mut downloader: Option<Arc<dyn DownloadFn>> = builder.download.clone();
    let mut videos: Vec<GeneratedVideo> = Vec::new();
    let mut warnings: Vec<Warning> = Vec::new();
    let mut responses: Vec<ResponseMetadata> = Vec::new();
    let mut provider_metadata = ProviderMetadata::new();
    for result in results.into_iter().flatten() {
        for video in result.videos {
            let reported = usable_media_type(&video.media_type).then_some(video.media_type);
            let (data, downloaded_media_type) = match video.data {
                FileData::Bytes { data } => (data, None),
                FileData::Url { url } => {
                    let download = match &downloader {
                        Some(download) => Arc::clone(download),
                        None => {
                            let default: Arc<dyn DownloadFn> =
                                Arc::new(DefaultDownloader::try_default()?);
                            downloader = Some(Arc::clone(&default));
                            default
                        }
                    };
                    let mut downloaded = download
                        .download(
                            vec![DownloadRequest {
                                url: url.clone(),
                                is_url_supported_by_model: false,
                            }],
                            cancellation.clone(),
                        )
                        .await?;
                    match downloaded.pop().flatten() {
                        Some(file) => (file.data, file.media_type.filter(usable_media_type)),
                        None => {
                            return Err(Error::download(
                                url,
                                None,
                                Some("the download function returned no data".into()),
                            ));
                        }
                    }
                }
                #[allow(unreachable_patterns, reason = "FileData is non-exhaustive")]
                _ => {
                    return Err(Error::invalid_data_content(
                        "video data must be bytes or a URL",
                        None,
                    ));
                }
            };
            let media_type = reported
                .or(downloaded_media_type)
                .or_else(|| detect_media_type_for(&data, "video"))
                .unwrap_or_else(|| MediaType::new(DEFAULT_VIDEO_MEDIA_TYPE));
            videos.push(GeneratedVideo { data, media_type });
        }
        warnings.extend(result.warnings);
        responses.push(result.response);
        if let Some(metadata) = &result.provider_metadata {
            merge_provider_metadata(&mut provider_metadata, metadata);
        }
    }
    if videos.is_empty() {
        return Err(Error::NoVideoGenerated { responses });
    }
    spans::log_warnings(&warnings, &identity);
    Ok(GenerateVideoResult {
        videos,
        warnings,
        responses,
        provider_metadata,
    })
}
