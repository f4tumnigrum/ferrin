//! Image generation and edits.

use bytes::Bytes;
use ferrin_spec::FileData;
use ferrin_spec::ImageModel;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::image_model::ImageFile;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageSize;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;
use url::Url;

use super::common::TestProvider;
use super::common::example_options;

fn png_file() -> ImageFile {
    ImageFile {
        data: FileData::Bytes {
            data: Bytes::from_static(b"\x89PNG\r\n\x1a\n...."),
        },
        media_type: Some("image/png".into()),
        provider_options: None,
    }
}

#[tokio::test]
async fn generate_decodes_base64_and_maps_usage() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/images/generations", "image", "generate");
    let model = test.provider.image("example-image");
    assert_eq!(model.max_images_per_call(), Some(10));
    let mut options = ImageOptions::new("A tiny red square");
    options.n = 2;
    options.size = Some(ImageSize::new(1024, 1024));
    options.aspect_ratio = Some(ferrin_spec::AspectRatio::new(1, 1));
    options.seed = Some(7);
    options.provider_options = example_options(json!({"quality": "high"}));
    let result = model.do_generate(options).await.unwrap();
    assert_eq!(result.images.len(), 1);
    assert!(result.images[0].data.starts_with(b"\x89PNG"));
    assert_eq!(
        result.images[0]
            .media_type
            .as_ref()
            .map(ferrin_spec::MediaType::as_str),
        Some("image/png")
    );
    let usage = result.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.output_tokens, Some(1000));
    assert_eq!(usage.total_tokens, Some(1012));
    assert_eq!(
        result.warnings,
        vec![
            Warning::unsupported_with_details(
                "aspectRatio",
                "This model does not support aspect ratio. Use `size` instead."
            ),
            Warning::unsupported("seed"),
        ]
    );
    let request = test.only_request().body_json().unwrap();
    assert_eq!(
        request,
        json!({
            "model": "example-image",
            "prompt": "A tiny red square",
            "n": 2,
            "size": "1024x1024",
            "quality": "high"
        })
    );
}

#[tokio::test]
async fn edits_send_a_multipart_form_with_images_and_mask() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/images/edits", "image", "edit");
    let mut options = ImageOptions::new("Add a hat");
    options.files = vec![png_file(), png_file()];
    options.mask = Some(png_file());
    options.provider_options = example_options(json!({"quality": "high", "background": null}));
    let result = test
        .provider
        .image("example-image")
        .do_generate(options)
        .await
        .unwrap();
    assert_eq!(result.images.len(), 1);
    let request = test.only_request();
    assert_eq!(request.path, "/v1/images/edits");
    assert!(
        request
            .header("content-type")
            .unwrap()
            .starts_with("multipart/form-data"),
    );
    let body = request.body_text();
    for expected in [
        "name=\"model\"\r\n\r\nexample-image",
        "name=\"prompt\"\r\n\r\nAdd a hat",
        "name=\"image[]\"; filename=\"image\"",
        "name=\"mask\"; filename=\"mask\"",
        "name=\"n\"\r\n\r\n1",
        "name=\"quality\"\r\n\r\nhigh",
    ] {
        assert!(body.contains(expected), "missing {expected:?} in {body}");
    }
    assert!(!body.contains("name=\"background\""), "{body}");
    assert_eq!(body.matches("name=\"image[]\"").count(), 2);
}

#[tokio::test]
async fn single_image_edits_use_the_image_field() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/images/edits", "image", "edit");
    let mut options = ImageOptions::new("Add a hat");
    options.files = vec![png_file()];
    test.provider
        .image("example-image")
        .do_generate(options)
        .await
        .unwrap();
    let body = test.only_request().body_text();
    assert!(
        body.contains("name=\"image\"; filename=\"image\""),
        "{body}"
    );
}

#[tokio::test]
async fn edit_files_must_be_inline_bytes() {
    let test = TestProvider::start().await;
    let mut options = ImageOptions::new("Add a hat");
    options.files = vec![ImageFile {
        data: FileData::Url {
            url: Url::parse("https://example.test/a.png").unwrap(),
        },
        media_type: None,
        provider_options: None,
    }];
    let error = test
        .provider
        .image("example-image")
        .do_generate(options)
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::UnsupportedFunctionality(_)),
        "{error:?}"
    );
    assert_eq!(test.server.received_count(), 0);
}
