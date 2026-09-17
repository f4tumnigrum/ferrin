use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use ferrin_core::Error;
use ferrin_core::generate_video;
use ferrin_core::video::PollConfig;
use ferrin_core::video::WebhookFactory;
use ferrin_core::video::WebhookHandle;
use ferrin_core::video::WebhookPayload;
use ferrin_spec::FileData;
use ferrin_spec::Headers;
use ferrin_spec::MediaType;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::VideoModel;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::video_model::VideoData;
use ferrin_spec::video_model::VideoOptions;
use ferrin_spec::video_model::VideoResult;
use ferrin_spec::video_model::VideoStartOptions;
use ferrin_spec::video_model::VideoStartResult;
use ferrin_spec::video_model::VideoStatusOptions;
use ferrin_spec::video_model::VideoStatusResult;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::common::lock;

struct VideoMock {
    provider: ProviderId,
    model_id: ModelId,
    sync: bool,
    operations: bool,
    webhook: bool,
    pending_polls: usize,
    fail_status: bool,
    hang_status: bool,
    retry_status: bool,
    status_token: Mutex<Option<CancellationToken>>,
    generate_calls: Mutex<Vec<VideoOptions>>,
    start_calls: Mutex<Vec<VideoStartOptions>>,
    status_calls: AtomicUsize,
}

fn video(data: &'static [u8]) -> VideoData {
    VideoData {
        data: FileData::bytes(Bytes::from_static(data)),
        media_type: MediaType::new("video/mp4"),
    }
}

impl VideoModel for VideoMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_videos_per_call(&self) -> Option<usize> {
        Some(1)
    }

    fn supports_generate(&self) -> bool {
        self.sync
    }

    fn do_generate(
        &self,
        options: VideoOptions,
    ) -> impl Future<Output = Result<VideoResult, ProviderError>> + Send {
        lock(&self.generate_calls).push(options);
        async move {
            Ok(VideoResult {
                videos: vec![video(b"sync-video")],
                warnings: Vec::new(),
                provider_metadata: None,
                response: ResponseMetadata::default(),
            })
        }
    }

    fn supports_operations(&self) -> bool {
        self.operations
    }

    fn do_start(
        &self,
        options: VideoStartOptions,
    ) -> impl Future<Output = Result<VideoStartResult, ProviderError>> + Send {
        lock(&self.start_calls).push(options);
        async move {
            Ok(VideoStartResult {
                operation: json!({ "id": "op-1" }),
                warnings: Vec::new(),
                provider_metadata: None,
                response: ResponseMetadata::default(),
            })
        }
    }

    fn do_status(
        &self,
        options: VideoStatusOptions,
    ) -> impl Future<Output = Result<VideoStatusResult, ProviderError>> + Send {
        let call = self.status_calls.fetch_add(1, Ordering::SeqCst);
        let pending = call < self.pending_polls;
        let fail = self.fail_status;
        *lock(&self.status_token) = Some(options.cancellation);
        async move {
            if self.hang_status {
                return std::future::pending().await;
            }
            if self.retry_status {
                return Err(ProviderError::ApiCall(Box::new(ApiCallError::new(
                    "temporary failure",
                    Url::parse("https://example.com").unwrap(),
                ))));
            }
            if fail {
                return Ok(VideoStatusResult::Error {
                    error: "content policy".to_owned(),
                    provider_metadata: None,
                    response: ResponseMetadata::default(),
                });
            }
            if pending {
                Ok(VideoStatusResult::Pending {
                    warnings: Vec::new(),
                    provider_metadata: None,
                    response: ResponseMetadata::default(),
                })
            } else {
                Ok(VideoStatusResult::Completed {
                    videos: vec![video(b"async-video")],
                    warnings: Vec::new(),
                    provider_metadata: None,
                    response: ResponseMetadata::default(),
                })
            }
        }
    }

    fn supports_webhook(&self) -> bool {
        self.webhook
    }
}

fn mock() -> VideoMock {
    VideoMock {
        provider: ProviderId::new("mock"),
        model_id: ModelId::new("video-mock"),
        sync: true,
        operations: false,
        webhook: false,
        pending_polls: 0,
        fail_status: false,
        hang_status: false,
        retry_status: false,
        status_token: Mutex::new(None),
        generate_calls: Mutex::new(Vec::new()),
        start_calls: Mutex::new(Vec::new()),
        status_calls: AtomicUsize::new(0),
    }
}

fn fast_poll() -> PollConfig {
    PollConfig {
        interval: Duration::from_millis(1),
        timeout: Duration::from_secs(5),
        max_attempts: None,
    }
}

#[tokio::test]
async fn synchronous_generation() {
    let model = Arc::new(mock());
    let result = generate_video(Arc::clone(&model), "a cat").await.unwrap();
    assert_eq!(result.videos.len(), 1);
    assert_eq!(result.video().unwrap().data.as_ref(), b"sync-video");
    assert_eq!(result.video().unwrap().media_type.as_str(), "video/mp4");
    assert_eq!(lock(&model.generate_calls).len(), 1);
}

#[tokio::test]
async fn polling_flow_when_requested() {
    let mut m = mock();
    m.operations = true;
    m.pending_polls = 2;
    let model = Arc::new(m);
    let result = generate_video(Arc::clone(&model), "a cat")
        .poll(fast_poll())
        .await
        .unwrap();
    assert_eq!(result.video().unwrap().data.as_ref(), b"async-video");
    assert_eq!(model.status_calls.load(Ordering::SeqCst), 3);
    assert!(lock(&model.generate_calls).is_empty());
    let starts = lock(&model.start_calls);
    assert!(starts[0].options.headers.contains("idempotency-key"));
    assert!(starts[0].webhook_url.is_none());
}

#[tokio::test]
async fn operations_flow_is_used_when_generate_is_unsupported() {
    let mut m = mock();
    m.sync = false;
    m.operations = true;
    let model = Arc::new(m);
    let result = generate_video(Arc::clone(&model), "a cat")
        .poll(fast_poll())
        .await
        .unwrap();
    assert_eq!(result.videos.len(), 1);
}

#[tokio::test]
async fn poll_falls_back_to_generate_without_operations() {
    let model = Arc::new(mock());
    let result = generate_video(Arc::clone(&model), "a cat")
        .poll(fast_poll())
        .await
        .unwrap();
    assert_eq!(result.video().unwrap().data.as_ref(), b"sync-video");
}

#[tokio::test]
async fn status_errors_fail_the_call() {
    let mut m = mock();
    m.operations = true;
    m.fail_status = true;
    let error = generate_video(Arc::new(m), "a cat")
        .poll(fast_poll())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("content policy"), "{error}");
}

#[tokio::test]
async fn polling_times_out() {
    let mut m = mock();
    m.operations = true;
    m.pending_polls = usize::MAX;
    let error = generate_video(Arc::new(m), "a cat")
        .poll(PollConfig {
            interval: Duration::from_millis(1),
            timeout: Duration::from_millis(20),
            max_attempts: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Timeout { .. }), "{error}");
}

#[tokio::test]
async fn webhook_flow_checks_status_once() {
    let mut m = mock();
    m.operations = true;
    m.webhook = true;
    let model = Arc::new(m);
    let factory: WebhookFactory = Arc::new(|| {
        Box::pin(async {
            Ok(WebhookHandle {
                url: Url::parse("https://hooks.example/v1").unwrap(),
                received: Box::pin(async {
                    Ok(WebhookPayload {
                        headers: Headers::new(),
                        body: json!({ "done": true }),
                    })
                }),
            })
        })
    });
    let result = generate_video(Arc::clone(&model), "a cat")
        .webhook(factory)
        .await
        .unwrap();
    assert_eq!(result.videos.len(), 1);
    assert_eq!(model.status_calls.load(Ordering::SeqCst), 1);
    let starts = lock(&model.start_calls);
    assert_eq!(
        starts[0].webhook_url.as_ref().map(Url::as_str),
        Some("https://hooks.example/v1")
    );
}

#[tokio::test]
async fn unsupported_models_fail() {
    let mut m = mock();
    m.sync = false;
    let error = generate_video(Arc::new(m), "a cat").await.unwrap_err();
    assert!(error.as_provider().is_some(), "{error}");
}

#[tokio::test(start_paused = true)]
async fn poll_deadline_bounds_hung_status_and_retry_backoff() {
    for retry_status in [false, true] {
        let mut m = mock();
        m.operations = true;
        m.hang_status = !retry_status;
        m.retry_status = retry_status;
        let model = Arc::new(m);
        let started = tokio::time::Instant::now();
        let error = tokio::time::timeout(
            Duration::from_secs(10),
            generate_video(Arc::clone(&model), "a cat").poll(PollConfig {
                interval: Duration::from_millis(1),
                timeout: Duration::from_millis(100),
                max_attempts: None,
            }),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(matches!(error, Error::Timeout { .. }), "{error}");
        assert_eq!(
            (started.elapsed(), model.status_calls.load(Ordering::SeqCst)),
            (Duration::from_millis(100), 1)
        );
        assert!(lock(&model.status_token).as_ref().unwrap().is_cancelled());
    }
}

#[tokio::test(start_paused = true)]
async fn webhook_status_request_obeys_the_poll_deadline() {
    let mut m = mock();
    m.operations = true;
    m.webhook = true;
    m.hang_status = true;
    let model = Arc::new(m);
    let error = tokio::time::timeout(
        Duration::from_secs(10),
        generate_video(Arc::clone(&model), "a cat")
            .poll(PollConfig {
                timeout: Duration::from_millis(100),
                ..fast_poll()
            })
            .webhook(Arc::new(|| {
                Box::pin(async {
                    Ok(WebhookHandle {
                        url: Url::parse("https://example.com/webhook").unwrap(),
                        received: Box::pin(async {
                            Ok(WebhookPayload {
                                headers: Headers::new(),
                                body: json!({}),
                            })
                        }),
                    })
                })
            })),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(matches!(error, Error::Timeout { .. }), "{error}");
    assert_eq!(model.status_calls.load(Ordering::SeqCst), 1);
    assert!(lock(&model.status_token).as_ref().unwrap().is_cancelled());
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_hung_status_request_returns_without_waiting_for_deadline() {
    use futures_util::FutureExt;
    use std::future::IntoFuture;
    let mut m = mock();
    m.operations = true;
    m.hang_status = true;
    let model = Arc::new(m);
    let token = CancellationToken::new();
    let mut call = generate_video(Arc::clone(&model), "a cat")
        .poll(PollConfig {
            interval: Duration::ZERO,
            ..fast_poll()
        })
        .cancellation(token.clone())
        .into_future();
    assert!(call.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_millis(1)).await;
    assert!(call.as_mut().now_or_never().is_none());
    assert_eq!(model.status_calls.load(Ordering::SeqCst), 1);
    token.cancel();
    let error = tokio::time::timeout(Duration::from_secs(1), call)
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(error, Error::Cancelled), "{error}");
}

#[tokio::test(start_paused = true)]
async fn cancellation_during_webhook_wait_is_immediate() {
    use tokio::sync::Notify;

    let mut m = mock();
    m.operations = true;
    m.webhook = true;
    let model = Arc::new(m);
    let token = CancellationToken::new();
    let entered = Arc::new(Notify::new());
    let pending = Arc::clone(&entered);
    let factory: WebhookFactory = Arc::new(move || {
        let pending = Arc::clone(&pending);
        Box::pin(async move {
            Ok(WebhookHandle {
                url: Url::parse("https://example.com/webhook").unwrap(),
                received: Box::pin(async move {
                    pending.notify_one();
                    std::future::pending().await
                }),
            })
        })
    });
    let start = tokio::time::Instant::now();
    let call = generate_video(Arc::clone(&model), "a cat")
        .webhook(factory)
        .cancellation(token.clone());
    let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(async { call.await }, async {
            entered.notified().await;
            token.cancel();
        })
    })
    .await
    .unwrap();
    let error = result.unwrap_err();
    assert!(matches!(error, Error::Cancelled), "{error}");
    assert_eq!(
        (start.elapsed(), model.status_calls.load(Ordering::SeqCst)),
        (Duration::ZERO, 0)
    );
}

#[tokio::test]
async fn frame_images_override_standalone_image_and_reference_inputs() {
    use ferrin_spec::Warning;
    use ferrin_spec::video_model::FrameImage;
    use ferrin_spec::video_model::FrameType;
    use ferrin_spec::video_model::VideoFile;
    let file = |payload: &'static [u8]| VideoFile {
        data: FileData::bytes(Bytes::from_static(payload)),
        media_type: Some(MediaType::new("image/png")),
        provider_options: None,
    };
    let model = Arc::new(mock());
    let first = file(b"first");
    let result = generate_video(Arc::clone(&model), "cat")
        .n(2)
        .image(file(b"standalone"))
        .frame_images(vec![FrameImage {
            image: first.clone(),
            frame_type: FrameType::FirstFrame,
        }])
        .input_references(vec![file(b"reference")])
        .await
        .unwrap();
    assert_eq!(
        result.warnings,
        vec![
            Warning::other(
                "inputReferences were ignored because frameImages were provided; frameImages and inputReferences cannot be combined."
            ),
            Warning::other(
                "prompt.image was ignored because a first_frame frameImage was provided; the first_frame frameImage takes precedence as the start image."
            ),
        ]
    );
    assert_eq!(
        lock(&model.generate_calls)
            .iter()
            .map(|call| (call.image.clone(), call.input_references.clone()))
            .collect::<Vec<_>>(),
        vec![(Some(first.clone()), vec![]), (Some(first), vec![])]
    );
}
