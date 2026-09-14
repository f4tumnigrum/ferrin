//! Veo video model: operation start and status polling.

use bytes::Bytes;
use ferrin_spec::AspectRatio;
use ferrin_spec::FileData;
use ferrin_spec::Headers;
use ferrin_spec::ImageSize;
use ferrin_spec::video_model::FrameImage;
use ferrin_spec::video_model::FrameType;
use ferrin_spec::video_model::VideoAspectRatio;
use ferrin_spec::video_model::VideoFile;
use ferrin_spec::video_model::VideoModel;
use ferrin_spec::video_model::VideoOptions;
use ferrin_spec::video_model::VideoStartOptions;
use ferrin_spec::video_model::VideoStatusOptions;
use ferrin_spec::video_model::VideoStatusResult;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::common::TestProvider;
use super::common::features;
use super::common::google_options;

const MODEL: &str = "veo-3.1-generate-preview";
const OPERATION: &str = "models/veo-3.1-generate-preview/operations/op-123";

fn image(data: &'static [u8]) -> VideoFile {
    VideoFile {
        data: FileData::Bytes {
            data: Bytes::from_static(data),
        },
        media_type: Some("image/jpeg".into()),
        provider_options: None,
    }
}

fn status_options(operation: serde_json::Value) -> VideoStatusOptions {
    VideoStatusOptions {
        operation,
        headers: Headers::new(),
        cancellation: CancellationToken::new(),
    }
}

#[tokio::test]
async fn start_posts_predict_long_running_and_returns_the_operation_name() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/models/veo-3.1-generate-preview:predictLongRunning",
        "video",
        "start",
    );
    let model = test.provider.video(MODEL);
    assert!(model.supports_operations());
    assert!(!model.supports_generate());
    assert_eq!(model.max_videos_per_call(), Some(4));
    let mut options = VideoOptions::new("A cat surfing");
    options.n = 2;
    options.aspect_ratio = Some(VideoAspectRatio::Ratio(AspectRatio::new(16, 9)));
    options.resolution = Some(ImageSize::new(1280, 720));
    options.duration = Some(8.0);
    options.seed = Some(42);
    options.fps = Some(24);
    options.frame_images = vec![
        FrameImage {
            image: image(b"first"),
            frame_type: FrameType::FirstFrame,
        },
        FrameImage {
            image: image(b"last"),
            frame_type: FrameType::LastFrame,
        },
    ];
    options.input_references = vec![image(b"ignored")];
    options.provider_options = google_options(json!({
        "personGeneration": "allow_adult",
        "negativePrompt": "rain",
        "pollIntervalMs": 1000,
        "enhancePrompt": true,
        "referenceImages": [{"gcsUri": "gs://bucket/ref.png"}]
    }));
    let result = model
        .do_start(VideoStartOptions {
            options,
            webhook_url: Some(Url::parse("https://example.test/hook").unwrap()),
        })
        .await
        .unwrap();
    assert_eq!(result.operation, json!({"operationName": OPERATION}));
    assert_eq!(features(&result.warnings), vec!["fps", "webhookUrl"]);
    let request = test.only_request().body_json().unwrap();
    insta::assert_json_snapshot!("video_start_request", request);
}

#[tokio::test]
async fn url_images_outside_gcs_are_dropped_with_a_warning() {
    let test = TestProvider::start().await;
    let mut options = VideoOptions::new("x");
    options.image = Some(VideoFile {
        data: FileData::Url {
            url: Url::parse("https://example.test/a.png").unwrap(),
        },
        media_type: None,
        provider_options: None,
    });
    options.input_references = vec![VideoFile {
        data: FileData::Url {
            url: Url::parse("gs://bucket/ref.png").unwrap(),
        },
        media_type: None,
        provider_options: None,
    }];
    let prepared = test.provider.video(MODEL).prepare_request(&options);
    assert_eq!(features(&prepared.warnings), vec!["URL-based image input"]);
    assert!(prepared.body["instances"][0].get("image").is_none());
    assert_eq!(
        prepared.body["instances"][0]["referenceImages"],
        json!([{"image": {"gcsUri": "gs://bucket/ref.png", "mimeType": "image/png"}, "referenceType": "asset"}])
    );
    assert_eq!(prepared.body["parameters"], json!({"sampleCount": 1}));
}

#[tokio::test]
async fn status_maps_pending_completed_and_failed_operations() {
    let test = TestProvider::start().await;
    let path = format!("/v1beta/{OPERATION}");
    let model = test.provider.video(MODEL);

    test.mount(Method::GET, &path, "video", "status-pending");
    let status = model
        .do_status(status_options(json!({"operationName": OPERATION})))
        .await
        .unwrap();
    assert!(matches!(status, VideoStatusResult::Pending { .. }));

    test.server.reset();
    test.mount(Method::GET, &path, "video", "status-done");
    let status = model
        .do_status(status_options(json!({"operationName": OPERATION})))
        .await
        .unwrap();
    let VideoStatusResult::Completed {
        videos,
        provider_metadata,
        ..
    } = status
    else {
        panic!("expected completed, got {status:?}");
    };
    assert_eq!(videos.len(), 2);
    assert_eq!(videos[0].media_type.as_str(), "video/mp4");
    let FileData::Url { url } = &videos[0].data else {
        panic!("expected url");
    };
    assert_eq!(
        url.as_str(),
        "https://generativelanguage.googleapis.com/v1beta/files/video-1:download?alt=media"
    );
    assert_eq!(
        provider_metadata.unwrap()["google"]["videos"],
        json!([
            {"uri": "https://generativelanguage.googleapis.com/v1beta/files/video-1:download?alt=media"},
            {"uri": "https://cdn.example.test/video-2.mp4"}
        ])
    );

    test.server.reset();
    test.mount(Method::GET, &path, "video", "status-error");
    let status = model
        .do_status(status_options(json!({"operationName": OPERATION})))
        .await
        .unwrap();
    let VideoStatusResult::Error { error, .. } = status else {
        panic!("expected error, got {status:?}");
    };
    assert_eq!(
        error,
        "Video generation failed: prompt violates the usage policy"
    );
}

#[tokio::test]
async fn same_origin_video_urls_get_the_api_key_appended() {
    let test = TestProvider::start().await;
    let uri = format!(
        "{}v1beta/files/video-1:download?alt=media",
        test.server.url()
    );
    test.mount_fixture(
        Method::GET,
        &format!("/v1beta/{OPERATION}"),
        Fixture::json(&json!({
            "name": OPERATION,
            "done": true,
            "response": {"generateVideoResponse": {"generatedSamples": [{"video": {"uri": uri}}]}}
        })),
    );
    let status = test
        .provider
        .video(MODEL)
        .do_status(status_options(json!({"operationName": OPERATION})))
        .await
        .unwrap();
    let VideoStatusResult::Completed { videos, .. } = status else {
        panic!("expected completed");
    };
    let FileData::Url { url } = &videos[0].data else {
        panic!("expected url");
    };
    assert_eq!(url.query(), Some("alt=media&key=test-key"));
}

#[tokio::test]
async fn operations_without_a_name_are_rejected() {
    let test = TestProvider::start().await;
    let error = test
        .provider
        .video(MODEL)
        .do_status(status_options(json!({})))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ferrin_spec::error::ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
}
