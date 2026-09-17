//! Files API: resumable upload with polling, metadata and delete.

use bytes::Bytes;
use ferrin_spec::Files;
use ferrin_spec::ProviderReference;
use ferrin_spec::error::ProviderError;
use ferrin_spec::files::FileReferenceOptions;
use ferrin_spec::files::UploadFileOptions;
use ferrin_testing::Fixture;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::google_options;

const FILE_URI: &str = "https://generativelanguage.googleapis.com/v1beta/files/abc-123";

fn reference(value: &str) -> ProviderReference {
    let mut reference = ProviderReference::new();
    reference.insert("google".to_owned(), value.to_owned());
    reference
}

fn mount_upload_start(test: &TestProvider) {
    let upload_url = format!("{}upload/session/xyz", test.server.url());
    test.mount_fixture(
        Method::POST,
        "/upload/v1beta/files",
        Fixture::complete(StatusCode::OK, "application/json", "{}")
            .with_header("x-goog-upload-url", &upload_url),
    );
}

#[tokio::test]
async fn upload_runs_the_resumable_protocol_and_polls_until_active() {
    let test = TestProvider::start().await;
    mount_upload_start(&test);
    test.mount(
        Method::POST,
        "/upload/session/xyz",
        "files",
        "upload-finalize",
    );
    test.mount(Method::GET, "/v1beta/files/abc-123", "files", "get-active");
    let files = test.provider.files();
    assert!(files.supports_get_file_metadata());
    assert!(files.supports_delete_file());
    assert!(!files.supports_download_file());
    let mut options = UploadFileOptions::new(Bytes::from_static(b"hello notes"), "text/plain");
    options.filename = Some("notes.txt".to_owned());
    options.provider_options =
        google_options(json!({"pollIntervalMs": 1, "displayName":"notes.txt"}));
    let result = files.upload_file(options).await.unwrap();
    assert_eq!(result.provider_reference, reference(FILE_URI));
    assert_eq!(result.filename.as_deref(), Some("notes.txt"));
    assert_eq!(
        result
            .media_type
            .as_ref()
            .map(ferrin_spec::MediaType::as_str),
        Some("text/plain")
    );
    assert_eq!(result.byte_size, Some(11));
    assert_eq!(super::common::features(&result.warnings), vec!["filename"]);
    assert_eq!(
        result.created_at.map(|t| t.to_rfc3339()),
        Some("2026-09-14T08:00:00+00:00".to_owned())
    );
    assert_eq!(
        result.expires_at.map(|t| t.to_rfc3339()),
        Some("2026-09-16T08:00:00+00:00".to_owned())
    );
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["google"]["state"], json!("ACTIVE"));
    assert_eq!(metadata["google"]["name"], json!("files/abc-123"));
    assert_eq!(metadata["google"]["sizeBytes"], json!("11"));
    assert_eq!(metadata["google"]["sha256Hash"], json!("ZGVhZGJlZWY="));

    let requests = test.server.received();
    assert_eq!(requests.len(), 3);
    let start = &requests[0];
    assert_eq!(start.path, "/upload/v1beta/files");
    assert_eq!(start.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(start.header("x-goog-upload-protocol"), Some("resumable"));
    assert_eq!(start.header("x-goog-upload-command"), Some("start"));
    assert_eq!(
        start.header("x-goog-upload-header-content-length"),
        Some("11")
    );
    assert_eq!(
        start.header("x-goog-upload-header-content-type"),
        Some("text/plain")
    );
    assert_eq!(
        start.body_json().unwrap(),
        json!({"file": {"display_name": "notes.txt"}})
    );
    let finalize = &requests[1];
    assert_eq!(finalize.path, "/upload/session/xyz");
    assert_eq!(finalize.header("x-goog-api-key"), None);
    assert_eq!(finalize.header("x-goog-upload-offset"), Some("0"));
    assert_eq!(
        finalize.header("x-goog-upload-command"),
        Some("upload, finalize")
    );
    assert_eq!(finalize.header("content-type"), Some("text/plain"));
    assert_eq!(finalize.body_text(), "hello notes");
    assert_eq!(requests[2].method, Method::GET);
    assert_eq!(requests[2].path, "/v1beta/files/abc-123");
}

#[tokio::test]
async fn upload_fails_on_timeout_failed_state_and_missing_upload_url() {
    let test = TestProvider::start().await;
    mount_upload_start(&test);
    test.mount(
        Method::POST,
        "/upload/session/xyz",
        "files",
        "upload-finalize",
    );
    let files = test.provider.files();
    let mut options = UploadFileOptions::new(Bytes::from_static(b"hello notes"), "text/plain");
    options.provider_options = google_options(json!({"pollTimeoutMs": 0}));
    let error = files.upload_file(options).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidArgument(_)),
        "{error:?}"
    );
    assert_eq!(test.server.received_count(), 0);

    test.server.reset();
    mount_upload_start(&test);
    test.mount_fixture(
        Method::POST,
        "/upload/session/xyz",
        Fixture::json(&json!({"file": {"name": "files/bad", "state": "FAILED"}})),
    );
    let error = files
        .upload_file(UploadFileOptions::new(
            Bytes::from_static(b"x"),
            "text/plain",
        ))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::ApiCall(_)), "{error:?}");

    test.server.reset();
    test.mount_fixture(
        Method::POST,
        "/upload/v1beta/files",
        Fixture::complete(StatusCode::OK, "application/json", "{}"),
    );
    let error = files
        .upload_file(UploadFileOptions::new(
            Bytes::from_static(b"x"),
            "text/plain",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::InvalidResponseData(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn metadata_and_delete_resolve_the_reference_to_the_file_name() {
    let test = TestProvider::start().await;
    test.mount(Method::GET, "/v1beta/files/abc-123", "files", "get-active");
    test.mount(Method::DELETE, "/v1beta/files/abc-123", "files", "delete");
    let files = test.provider.files();
    let result = files
        .get_file_metadata(FileReferenceOptions::new(reference(FILE_URI)))
        .await
        .unwrap();
    assert_eq!(result.byte_size, Some(11));
    assert_eq!(result.provider_reference, reference(FILE_URI));
    let deleted = files
        .delete_file(FileReferenceOptions::new(reference("files/abc-123")))
        .await
        .unwrap();
    assert!(deleted.deleted);
    assert_eq!(deleted.provider_reference, reference("files/abc-123"));
    let requests = test.server.received();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(requests[1].method, Method::DELETE);
    assert_eq!(requests[1].header("x-goog-api-key"), Some("test-key"));

    let mut foreign = ProviderReference::new();
    foreign.insert("openai".to_owned(), "file-1".to_owned());
    let error = files
        .get_file_metadata(FileReferenceOptions::new(foreign))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::NoSuchProviderReference(_)),
        "{error:?}"
    );
}
