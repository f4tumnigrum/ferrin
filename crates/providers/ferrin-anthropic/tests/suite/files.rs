//! Files API.

use bytes::Bytes;
use ferrin_spec::Files;
use ferrin_spec::ProviderReference;
use ferrin_spec::files::UploadFileOptions;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;

#[tokio::test]
async fn upload_sends_multipart_with_the_files_beta_and_maps_metadata() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/files", "files", "upload");
    let files = test.provider.files();
    assert!(!files.supports_get_file_metadata());
    assert!(!files.supports_download_file());
    assert!(!files.supports_delete_file());
    let mut options = UploadFileOptions::new(Bytes::from_static(b"hello notes"), "text/plain");
    options.filename = Some("notes.txt".to_owned());
    let result = files.upload_file(options).await.unwrap();
    let mut reference = ProviderReference::new();
    reference.insert("anthropic".to_owned(), "file_abc123".to_owned());
    assert_eq!(result.provider_reference, reference);
    assert_eq!(result.filename.as_deref(), Some("notes.txt"));
    assert_eq!(
        result
            .media_type
            .as_ref()
            .map(ferrin_spec::MediaType::as_str),
        Some("text/plain")
    );
    assert_eq!(result.byte_size, Some(11));
    assert!(result.created_at.is_some());
    assert!(result.expires_at.is_none());
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["anthropic"]["mimeType"], json!("text/plain"));
    assert_eq!(metadata["anthropic"]["sizeBytes"], json!(11));
    assert_eq!(metadata["anthropic"]["downloadable"], json!(true));
    let request = test.only_request();
    assert_eq!(
        request.header("anthropic-beta"),
        Some("files-api-2025-04-14")
    );
    let body = request.body_text();
    assert!(
        body.contains("name=\"file\"; filename=\"notes.txt\""),
        "{body}"
    );
    assert!(body.contains("Content-Type: text/plain"), "{body}");
    assert!(body.contains("hello notes"), "{body}");
}
