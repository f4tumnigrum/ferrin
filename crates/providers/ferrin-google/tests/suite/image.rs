//! Gemini image model built on `generateContent`.

use ferrin_spec::AspectRatio;
use ferrin_spec::FileData;
use ferrin_spec::ImageModel;
use ferrin_spec::ImageSize;
use ferrin_spec::error::ProviderError;
use ferrin_spec::image_model::ImageFile;
use ferrin_spec::image_model::ImageOptions;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::features;
use super::common::google_options;

#[tokio::test]
async fn generate_requests_the_image_modality_and_collects_image_files() {
    let test = TestProvider::start().await;
    test.mount(
        Method::POST,
        "/v1beta/models/gemini-2.5-flash-image:generateContent",
        "generate",
        "inline-image",
    );
    let model = test.provider.image("gemini-2.5-flash-image");
    assert_eq!(model.max_images_per_call(), Some(10));
    let mut options = ImageOptions::new("A red square");
    options.aspect_ratio = Some(AspectRatio::new(16, 9));
    options.seed = Some(3);
    options.files = vec![ImageFile {
        data: FileData::Bytes {
            data: bytes::Bytes::from_static(b"\x89PNG"),
        },
        media_type: Some("image/png".into()),
        provider_options: None,
    }];
    options.provider_options = google_options(json!({
        "imageConfig": {"imageSize": "2K"},
        "googleSearch": {},
        "responseModalities": ["TEXT"]
    }));
    let result = model.do_generate(options).await.unwrap();
    assert_eq!(result.images.len(), 1);
    assert_eq!(
        result.images[0]
            .media_type
            .as_ref()
            .map(ferrin_spec::MediaType::as_str),
        Some("image/png")
    );
    assert_eq!(result.images[0].data.len(), 70);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(result.is_retryable, None);
    let usage = result.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(11));
    assert_eq!(usage.output_tokens, Some(1290));
    assert_eq!(usage.total_tokens, Some(1301));
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["google"]["images"], json!([{}]));
    assert!(metadata["google"]["usageMetadata"].is_object());
    let request = test.only_request().body_json().unwrap();
    insta::assert_json_snapshot!("image_request", request);
}

#[tokio::test]
async fn unsupported_arguments_are_rejected_or_warned() {
    let test = TestProvider::start().await;
    let error = test
        .provider
        .image("imagen-4.0-generate-001")
        .prepare_call(&ImageOptions::new("x"))
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
    let model = test.provider.image("gemini-2.5-flash-image");
    let mut options = ImageOptions::new("x");
    options.n = 2;
    assert!(matches!(
        model.prepare_call(&options).unwrap_err(),
        ProviderError::InvalidArgument(_)
    ));
    let mut options = ImageOptions::new("x");
    options.mask = Some(ImageFile {
        data: FileData::Bytes {
            data: bytes::Bytes::from_static(b"m"),
        },
        media_type: None,
        provider_options: None,
    });
    assert!(matches!(
        model.prepare_call(&options).unwrap_err(),
        ProviderError::InvalidArgument(_)
    ));
    let mut options = ImageOptions::new("x");
    options.size = Some(ImageSize::new(1024, 1024));
    let (_, warnings) = model.prepare_call(&options).unwrap();
    assert_eq!(features(&warnings), vec!["size"]);
}
