use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::files::get_file_metadata;
use ferrin_core::upload_file;
use ferrin_core::upload_skill;
use ferrin_spec::Files;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderReference;
use ferrin_spec::Skills;
use ferrin_spec::error::ProviderError;
use ferrin_spec::files::UploadData;
use ferrin_spec::files::UploadFileOptions;
use ferrin_spec::files::UploadFileResult;
use ferrin_spec::skills::SkillFile;
use ferrin_spec::skills::SkillFileData;
use ferrin_spec::skills::UploadSkillOptions;
use ferrin_spec::skills::UploadSkillResult;
use pretty_assertions::assert_eq;

use super::common::PNG_BYTES;
use super::common::lock;

struct FilesMock {
    provider: ProviderId,
    uploads: Mutex<Vec<(String, Option<String>, usize)>>,
}

impl Files for FilesMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn upload_file(
        &self,
        options: UploadFileOptions,
    ) -> impl Future<Output = Result<UploadFileResult, ProviderError>> + Send {
        let size = match &options.data {
            UploadData::Bytes(bytes) => bytes.len(),
            UploadData::Text(text) => text.len(),
            _ => 0,
        };
        lock(&self.uploads).push((
            options.media_type.as_str().to_owned(),
            options.filename.clone(),
            size,
        ));
        let media_type = options.media_type.clone();
        async move {
            Ok(UploadFileResult {
                provider_reference: reference("file-1"),
                media_type: Some(media_type),
                filename: options.filename,
                byte_size: Some(size as u64),
                created_at: None,
                expires_at: None,
                provider_metadata: None,
                warnings: Vec::new(),
            })
        }
    }
}

struct SkillsMock {
    provider: ProviderId,
    uploads: Mutex<Vec<UploadSkillOptions>>,
}

impl Skills for SkillsMock {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn upload_skill(
        &self,
        options: UploadSkillOptions,
    ) -> impl Future<Output = Result<UploadSkillResult, ProviderError>> + Send {
        let title = options.display_title.clone();
        lock(&self.uploads).push(options);
        async move {
            Ok(UploadSkillResult {
                provider_reference: reference("skill-1"),
                display_title: title,
                name: Some("weather".to_owned()),
                description: None,
                latest_version: Some("1".to_owned()),
                provider_metadata: None,
                warnings: Vec::new(),
            })
        }
    }
}

fn reference(id: &str) -> ProviderReference {
    ProviderReference::from([("id".to_owned(), id.to_owned())])
}

fn files() -> Arc<FilesMock> {
    Arc::new(FilesMock {
        provider: ProviderId::new("mock"),
        uploads: Mutex::new(Vec::new()),
    })
}

#[tokio::test]
async fn uploads_bytes_with_detected_media_types() {
    let api = files();
    upload_file(Arc::clone(&api), PNG_BYTES.to_vec())
        .filename("a.png")
        .await
        .unwrap();
    upload_file(Arc::clone(&api), b"hello world".to_vec())
        .await
        .unwrap();
    upload_file(Arc::clone(&api), vec![0u8, 1, 2, 3])
        .await
        .unwrap();
    upload_file(Arc::clone(&api), "text".to_owned())
        .await
        .unwrap();
    let result = upload_file(Arc::clone(&api), b"%PDF-1.7".to_vec())
        .media_type("application/pdf")
        .filename("spec.pdf")
        .await
        .unwrap();
    assert_eq!(
        result.provider_reference.get("id").map(String::as_str),
        Some("file-1")
    );
    let uploads = lock(&api.uploads);
    assert_eq!(uploads[0].0, "image/png");
    assert_eq!(uploads[0].1.as_deref(), Some("a.png"));
    assert_eq!(uploads[1].0, "text/plain");
    assert_eq!(uploads[2].0, "application/octet-stream");
    assert_eq!(uploads[3].0, "text/plain");
    assert_eq!(uploads[4].0, "application/pdf");
    assert_eq!(uploads[4].1.as_deref(), Some("spec.pdf"));
}

#[tokio::test]
async fn unsupported_file_operations_fail() {
    let error = get_file_metadata(files(), reference("file-1"))
        .await
        .unwrap_err();
    assert!(error.as_provider().is_some(), "{error}");
}

#[tokio::test]
async fn uploads_skills() {
    let api = Arc::new(SkillsMock {
        provider: ProviderId::new("mock"),
        uploads: Mutex::new(Vec::new()),
    });
    let result = upload_skill(
        Arc::clone(&api),
        vec![SkillFile {
            path: "SKILL.md".to_owned(),
            data: SkillFileData::Text {
                text: "# Weather".to_owned(),
            },
        }],
    )
    .display_title("Weather")
    .await
    .unwrap();
    assert_eq!(result.display_title.as_deref(), Some("Weather"));
    let uploads = lock(&api.uploads);
    assert_eq!(uploads[0].files.len(), 1);
    assert_eq!(uploads[0].files[0].path, "SKILL.md");
}
