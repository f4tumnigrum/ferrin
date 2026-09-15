//! Files API.

use bytes::Bytes;
use ferrin_spec::Files;
use ferrin_spec::ProviderReference;
use ferrin_spec::files::FileReferenceOptions;
use ferrin_spec::files::UploadFileOptions;
use futures_util::StreamExt;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::openai_options;

fn reference(id: &str) -> ProviderReference {
    let mut reference = ProviderReference::new();
    reference.insert("openai".to_owned(), id.to_owned());
    reference
}

#[tokio::test]
async fn upload_sends_multipart_and_maps_metadata() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/files", "files", "upload");
    let files = test.provider.files();
    let mut options = UploadFileOptions::new(Bytes::from_static(b"hello notes"), "text/plain");
    options.filename = Some("notes.txt".to_owned());
    options.provider_options = openai_options(json!({
        "purpose": "user_data",
        "expiresAfter": {"anchor": "created_at", "seconds": 86400}
    }));
    let result = files.upload_file(options).await.unwrap();
    assert_eq!(result.provider_reference, reference("file-abc123"));
    assert_eq!(result.filename.as_deref(), Some("notes.txt"));
    assert_eq!(result.byte_size, Some(11));
    assert!(result.created_at.is_some());
    assert!(result.expires_at.is_some());
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["openai"]["purpose"], json!("assistants"));
    let body = test.only_request().body_text();
    for expected in [
        "name=\"file\"; filename=\"notes.txt\"",
        "name=\"purpose\"\r\n\r\nuser_data",
        "name=\"expires_after[anchor]\"\r\n\r\ncreated_at",
        "name=\"expires_after[seconds]\"\r\n\r\n86400",
    ] {
        assert!(body.contains(expected), "missing {expected:?} in {body}");
    }
}

#[tokio::test]
async fn get_download_and_delete_use_the_file_id() {
    let test = TestProvider::start().await;
    test.mount(Method::GET, "/v1/files/file-abc123", "files", "get");
    test.mount_fixture(
        Method::GET,
        "/v1/files/file-abc123/content",
        ferrin_testing::Fixture::complete(
            http::StatusCode::OK,
            "text/plain",
            Bytes::from_static(b"hello notes"),
        ),
    );
    test.mount(Method::DELETE, "/v1/files/file-abc123", "files", "delete");
    let files = test.provider.files();
    assert!(files.supports_get_file_metadata());
    assert!(files.supports_download_file());

    let metadata = files
        .get_file_metadata(FileReferenceOptions::new(reference("file-abc123")))
        .await
        .unwrap();
    assert_eq!(metadata.filename.as_deref(), Some("notes.txt"));

    let download = files
        .download_file(FileReferenceOptions::new(reference("file-abc123")))
        .await
        .unwrap();
    assert_eq!(
        download
            .media_type
            .as_ref()
            .map(ferrin_spec::MediaType::as_str),
        Some("text/plain")
    );
    let chunks: Vec<Bytes> = download.content.map(|chunk| chunk.unwrap()).collect().await;
    assert_eq!(chunks.concat(), b"hello notes");

    let deleted = files
        .delete_file(FileReferenceOptions::new(reference("file-abc123")))
        .await
        .unwrap();
    assert!(deleted.deleted);
    assert_eq!(test.server.received_count(), 3);
}

#[tokio::test]
async fn foreign_references_are_rejected() {
    let test = TestProvider::start().await;
    let mut reference = ProviderReference::new();
    reference.insert("other".to_owned(), "x".to_owned());
    let error = test
        .provider
        .files()
        .get_file_metadata(FileReferenceOptions::new(reference))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            ferrin_spec::error::ProviderError::NoSuchProviderReference(_)
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn uploaded_files_roundtrip_with_custom_provider_name() {
    use ferrin_spec::CallOptions;
    use ferrin_spec::FileData;
    use ferrin_spec::PromptMessage;
    use ferrin_spec::language_model::prompt::FilePart;
    use ferrin_spec::language_model::prompt::UserPromptPart;

    let test = TestProvider::start_with(|mut settings| {
        settings.name = Some("azure".to_owned());
        settings
    })
    .await;
    test.mount(Method::POST, "/v1/files", "files", "upload");
    let uploaded = test
        .provider
        .files()
        .upload_file(UploadFileOptions::new(
            Bytes::from_static(b"%PDF-1.4 test"),
            "application/pdf",
        ))
        .await
        .unwrap();
    let options = CallOptions::new(vec![PromptMessage::user(vec![UserPromptPart::File(
        FilePart::new(
            FileData::Reference {
                reference: uploaded.provider_reference,
            },
            "application/pdf",
        ),
    )])]);
    let responses = ferrin_openai::responses::request::prepare_request(
        test.provider.config(),
        "gpt-4.1",
        &options,
    )
    .unwrap();
    let chat = test
        .provider
        .chat("gpt-4.1")
        .prepare_request(&options)
        .unwrap();
    assert_eq!(
        serde_json::to_value(responses.body).unwrap()["input"][0]["content"],
        json!([
            {"type": "input_file", "file_id": "file-abc123"}
        ])
    );
    assert_eq!(
        serde_json::to_value(chat.body).unwrap()["messages"][0]["content"],
        json!([
            {"type": "file", "file": {"file_id": "file-abc123"}}
        ])
    );
}
