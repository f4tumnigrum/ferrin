//! Images API.

use ferrin_spec::ImageModel;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageSize;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::openai_options;

#[tokio::test]
async fn generate_decodes_base64_and_maps_metadata() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/images/generations", "image", "generate");
    let model = test.provider.image("gpt-image-1");
    assert_eq!(model.max_images_per_call(), Some(10));
    let mut options = ImageOptions::new("A tiny red square");
    options.n = 1;
    options.size = Some(ImageSize::new(1024, 1024));
    options.aspect_ratio = Some(ferrin_spec::AspectRatio::new(1, 1));
    options.provider_options = openai_options(json!({"quality": "high", "outputFormat": "png"}));
    let result = model.do_generate(options).await.unwrap();
    assert_eq!(result.images.len(), 1);
    assert!(result.images[0].data.starts_with(b"\x89PNG"));
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(
        metadata["openai"]["images"][0]["revisedPrompt"],
        json!("A tiny red square")
    );
    let usage = result.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.output_tokens, Some(1000));
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["model"], json!("gpt-image-1"));
    assert_eq!(request["size"], json!("1024x1024"));
    assert_eq!(request["quality"], json!("high"));
    assert_eq!(request["output_format"], json!("png"));
    assert!(request.get("response_format").is_none());
}

#[tokio::test]
async fn dall_e_requests_base64_explicitly() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/images/generations", "image", "generate");
    let model = test.provider.image("dall-e-3");
    assert_eq!(model.max_images_per_call(), Some(1));
    model
        .do_generate(ImageOptions::new("A tiny red square"))
        .await
        .unwrap();
    let request = test.only_request().body_json().unwrap();
    assert_eq!(request["response_format"], json!("b64_json"));
}

#[tokio::test]
async fn image_edits_use_model_specific_file_fields_and_response_format() {
    use ferrin_spec::FileData;
    use ferrin_spec::image_model::ImageFile;

    for (model, image_field, response_format) in [
        ("dall-e-2", "image", true),
        ("gpt-image-1", "image[]", false),
    ] {
        let test = TestProvider::start().await;
        test.mount(Method::POST, "/v1/images/edits", "image", "generate");
        let mut options = ImageOptions::new("A tiny red square");
        options.files.push(ImageFile {
            data: FileData::Bytes {
                data: bytes::Bytes::from_static(b"image bytes"),
            },
            media_type: Some("image/png".into()),
            provider_options: None,
        });
        let result = test
            .provider
            .image(model)
            .do_generate(options)
            .await
            .unwrap();
        let body = test.only_request().body_text();
        assert!(
            body.contains(&format!("name=\"{image_field}\"; filename=\"image\"")),
            "{body}"
        );
        assert_eq!(
            body.contains("name=\"response_format\"\r\n\r\nb64_json"),
            response_format
        );
        assert_eq!(result.images.len(), 1);
        assert!(result.images[0].data.starts_with(b"\x89PNG"));
    }
}
