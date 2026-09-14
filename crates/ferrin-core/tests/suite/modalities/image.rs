use std::sync::Arc;

use ferrin_core::Error;
use ferrin_core::generate_image;
use ferrin_spec::ImageSize;
use pretty_assertions::assert_eq;

use super::common::ImageMock;
use super::common::fast_retry;

#[tokio::test]
async fn generates_one_image_and_detects_the_media_type() {
    let model = Arc::new(ImageMock::new());
    let result = generate_image(Arc::clone(&model), "a lighthouse")
        .size(ImageSize::new(1024, 1024))
        .seed(7)
        .await
        .unwrap();
    assert_eq!(result.images.len(), 1);
    assert_eq!(result.image().unwrap().media_type.as_str(), "image/png");
    assert_eq!(result.calls.len(), 1);
    assert_eq!(result.usage.output_tokens, Some(1));
    let calls = model.calls.lock().unwrap();
    assert_eq!(calls[0].prompt.as_deref(), Some("a lighthouse"));
    assert_eq!(calls[0].size, Some(ImageSize::new(1024, 1024)));
    assert_eq!(calls[0].seed, Some(7));
    assert!(
        calls[0]
            .headers
            .get_str("user-agent")
            .is_some_and(|ua| ua.starts_with("ferrin/"))
    );
}

#[tokio::test]
async fn splits_n_across_calls_by_the_per_call_limit() {
    let mut mock = ImageMock::new();
    mock.max_per_call = Some(2);
    let model = Arc::new(mock);
    let result = generate_image(Arc::clone(&model), "x").n(5).await.unwrap();
    assert_eq!(result.images.len(), 5);
    assert_eq!(result.calls.len(), 3);
    assert_eq!(model.call_counts(), vec![2, 2, 1]);
    assert_eq!(result.usage.output_tokens, Some(5));
    assert_eq!(result.usage.input_tokens, Some(3));
}

#[tokio::test]
async fn empty_results_are_retried() {
    let mut mock = ImageMock::new();
    mock.empty_first = 1;
    let model = Arc::new(mock);
    let result = generate_image(Arc::clone(&model), "x")
        .retry(fast_retry(2))
        .await
        .unwrap();
    assert_eq!(result.images.len(), 1);
    assert_eq!(result.calls.len(), 2);
    assert_eq!(model.call_counts(), vec![1, 1]);
}

#[tokio::test]
async fn all_empty_results_fail_with_no_image_generated() {
    let mut mock = ImageMock::new();
    mock.empty_first = 10;
    let model = Arc::new(mock);
    let error = generate_image(Arc::clone(&model), "x")
        .retry(fast_retry(1))
        .await
        .unwrap_err();
    match error {
        Error::NoImageGenerated { responses } => assert_eq!(responses.len(), 2),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn non_retryable_empty_results_are_not_retried() {
    let mut mock = ImageMock::new();
    mock.empty_first = 10;
    mock.empty_not_retryable = true;
    let model = Arc::new(mock);
    let error = generate_image(Arc::clone(&model), "x")
        .retry(fast_retry(3))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NoImageGenerated { .. }));
    assert_eq!(model.call_counts(), vec![1]);
}

#[tokio::test]
async fn zero_images_is_an_invalid_argument() {
    let error = generate_image(Arc::new(ImageMock::new()), "x")
        .n(0)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidArgument { .. }));
}
