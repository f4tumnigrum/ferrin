//! Resource request parity with the pinned reference adapter.

use bytes::Bytes;
use ferrin_spec::Batch;
use ferrin_spec::Files;
use ferrin_spec::ProviderReference;
use ferrin_spec::batch::BatchOperationOptions;
use ferrin_spec::error::ProviderError;
use ferrin_spec::files::FileReferenceOptions;
use ferrin_spec::files::UploadFileOptions;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::openai_options;

fn reference(id: &str) -> FileReferenceOptions {
    FileReferenceOptions::new(ProviderReference::from_iter([("openai".into(), id.into())]))
}

#[tokio::test]
async fn files_encode_resource_ids_for_metadata_download_and_delete() {
    for (id, encoded) in [
        (".", "%252E"),
        ("..", "%252E%252E"),
        ("folder/file ?#%", "folder%2Ffile%20%3F%23%25"),
    ] {
        let test = TestProvider::start().await;
        let path = format!("/v1/files/{encoded}");
        test.mount_fixture(Method::GET, &path, Fixture::json(&json!({"id": id})));
        test.mount_fixture(
            Method::GET,
            &format!("{path}/content"),
            Fixture::complete(
                http::StatusCode::OK,
                "text/plain",
                Bytes::from_static(b"file"),
            ),
        );
        test.mount_fixture(
            Method::DELETE,
            &path,
            Fixture::json(&json!({"id": id, "deleted": true})),
        );
        let files = test.provider.files();
        files.get_file_metadata(reference(id)).await.unwrap();
        files.download_file(reference(id)).await.unwrap();
        files.delete_file(reference(id)).await.unwrap();
        let requests = test.server.received();
        assert_eq!(
            requests
                .iter()
                .map(|request| (request.path.as_str(), request.query.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                (path.as_str(), None),
                (format!("{path}/content").as_str(), None),
                (path.as_str(), None)
            ]
        );
    }
}

#[tokio::test]
async fn blank_file_ids_fail_before_requests() {
    let test = TestProvider::start().await;
    for id in ["", "   "] {
        let files = test.provider.files();
        for error in [
            files.get_file_metadata(reference(id)).await.unwrap_err(),
            files.download_file(reference(id)).await.unwrap_err(),
            files.delete_file(reference(id)).await.unwrap_err(),
        ] {
            assert!(
                matches!(error, ProviderError::InvalidArgument(_)),
                "{error:?}"
            );
        }
    }
    assert_eq!(test.server.received_count(), 0);
}

#[tokio::test]
async fn numeric_file_expiry_and_missing_response_filename_follow_reference() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1/files",
        Fixture::json(&json!({"id": "file-id"})),
    );
    let mut options = UploadFileOptions::new(Bytes::from_static(b"file"), "text/plain");
    options.filename = Some("notes.txt".into());
    options.provider_options = openai_options(json!({"expiresAfter": 3600}));
    let result = test.provider.files().upload_file(options).await.unwrap();
    assert_eq!(result.filename, Some("notes.txt".into()));
    let body = test.only_request().body_text();
    assert!(body.contains("name=\"expires_after[anchor]\"\r\n\r\ncreated_at"));
    assert!(body.contains("name=\"expires_after[seconds]\"\r\n\r\n3600"));
    let unnamed = TestProvider::start().await;
    unnamed.mount_fixture(
        Method::POST,
        "/v1/files",
        Fixture::json(&json!({"id": "file-id"})),
    );
    unnamed
        .provider
        .files()
        .upload_file(UploadFileOptions::new(
            Bytes::from_static(b"file"),
            "text/plain",
        ))
        .await
        .unwrap();
    assert!(
        unnamed
            .only_request()
            .body_text()
            .contains("filename=\"blob\"")
    );
}

#[tokio::test]
async fn batch_status_and_cancellation_encode_ids_as_one_segment() {
    let test = TestProvider::start().await;
    let id = "batch/with ?#%";
    let path = "/v1/batches/batch%2Fwith%20%3F%23%25";
    let response = json!({"id": id, "status": "cancelled"});
    test.mount_fixture(Method::GET, path, Fixture::json(&response));
    test.mount_fixture(
        Method::POST,
        &format!("{path}/cancel"),
        Fixture::json(&response),
    );
    let batch = test.provider.batch();
    batch
        .do_get_batch_status(BatchOperationOptions::new(id))
        .await
        .unwrap();
    batch
        .do_cancel_batch(BatchOperationOptions::new(id))
        .await
        .unwrap();
    assert_eq!(
        test.server
            .received()
            .iter()
            .map(|request| request.path.clone())
            .collect::<Vec<_>>(),
        vec![path.to_owned(), format!("{path}/cancel")]
    );
}
