//! Files service (`<name>.files`).

use bytes::Bytes;
use bytes::BytesMut;
use ferrin_provider_util::MultipartForm;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::binary_stream_response_handler;
use ferrin_provider_util::http::delete;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_form;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_provider_util::provider_reference::resolve_provider_reference;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderReference;
use ferrin_spec::error::ProviderError;
use ferrin_spec::files::DeleteFileResult;
use ferrin_spec::files::DownloadFileResult;
use ferrin_spec::files::FileMetadataResult;
use ferrin_spec::files::FileReferenceOptions;
use ferrin_spec::files::Files;
use ferrin_spec::files::UploadData;
use ferrin_spec::files::UploadFileOptions;
use ferrin_spec::files::UploadFileResult;
use futures_util::StreamExt;
use futures_util::TryStreamExt;
use serde::Deserialize;

use crate::config::SharedConfig;
use crate::embedding::compact;
use crate::embedding::provider_metadata;
use crate::error::failed_response_handler;

/// Default upload purpose.
pub const DEFAULT_PURPOSE: &str = "assistants";

/// Provider options of file uploads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesProviderOptions {
    /// Upload purpose (`assistants`, `batch`, `user_data`, `vision`, ...).
    #[serde(default)]
    pub purpose: Option<String>,
    /// Expiry in seconds after creation.
    #[serde(default)]
    pub expires_after: Option<ExpiresAfter>,
}

/// File expiry configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpiresAfter {
    /// Anchor (`created_at`).
    #[serde(default)]
    pub anchor: Option<String>,
    /// Seconds after the anchor.
    pub seconds: u64,
}

/// A file object returned by the API.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiFileObject {
    /// File id.
    pub id: String,
    /// Size in bytes.
    #[serde(default)]
    pub bytes: Option<u64>,
    /// Creation time (epoch seconds).
    #[serde(default)]
    pub created_at: Option<i64>,
    /// File name.
    #[serde(default)]
    pub filename: Option<String>,
    /// Purpose.
    #[serde(default)]
    pub purpose: Option<String>,
    /// Processing status.
    #[serde(default)]
    pub status: Option<String>,
    /// Expiry time (epoch seconds).
    #[serde(default)]
    pub expires_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct DeleteResponse {
    id: String,
    #[serde(default)]
    deleted: bool,
}

/// Files service backed by `/files`.
#[derive(Debug, Clone)]
pub struct OpenAiFiles {
    config: SharedConfig,
    provider: ProviderId,
}

impl OpenAiFiles {
    /// Creates the service.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: config.provider_id("files"),
            config,
        }
    }

    fn file_id<'a>(&self, reference: &'a ProviderReference) -> Result<&'a str, ProviderError> {
        Ok(resolve_provider_reference(reference, &self.config.name)?)
    }

    fn reference(&self, id: &str) -> ProviderReference {
        let mut reference = ProviderReference::new();
        reference.insert(self.config.name.clone(), id.to_owned());
        reference
    }

    fn to_result(
        &self,
        file: &OpenAiFileObject,
        media_type: Option<MediaType>,
    ) -> UploadFileResult {
        let mut meta = JsonObject::new();
        meta.insert(
            "filename".to_owned(),
            file.filename
                .clone()
                .map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "purpose".to_owned(),
            file.purpose
                .clone()
                .map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "bytes".to_owned(),
            file.bytes.map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "createdAt".to_owned(),
            file.created_at.map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "status".to_owned(),
            file.status.clone().map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "expiresAt".to_owned(),
            file.expires_at.map_or(JsonValue::Null, JsonValue::from),
        );
        UploadFileResult {
            provider_reference: self.reference(&file.id),
            media_type,
            filename: file.filename.clone(),
            byte_size: file.bytes,
            created_at: file
                .created_at
                .and_then(|s| chrono::DateTime::from_timestamp(s, 0)),
            expires_at: file
                .expires_at
                .and_then(|s| chrono::DateTime::from_timestamp(s, 0)),
            provider_metadata: Some(provider_metadata(
                &self.config.provider_options_key,
                compact(meta),
            )),
            warnings: Vec::new(),
        }
    }
}

async fn collect(data: UploadData) -> Result<Bytes, ProviderError> {
    match data {
        UploadData::Bytes(bytes) => Ok(bytes),
        UploadData::Text(text) => Ok(Bytes::from(text)),
        UploadData::Stream(stream) => {
            let mut buffer = BytesMut::new();
            let mut stream = stream;
            while let Some(chunk) = stream.next().await {
                buffer.extend_from_slice(&chunk?);
            }
            Ok(buffer.freeze())
        }
        #[allow(unreachable_patterns, reason = "UploadData may grow")]
        _ => Err(ProviderError::unsupported("upload data type")),
    }
}

impl Files for OpenAiFiles {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    async fn upload_file(
        &self,
        options: UploadFileOptions,
    ) -> Result<UploadFileResult, ProviderError> {
        let openai = parse_provider_options::<FilesProviderOptions>(
            &self.config.provider_options_key,
            &options.provider_options,
        )?
        .unwrap_or_default();
        let data = collect(options.data).await?;
        let filename = options
            .filename
            .clone()
            .unwrap_or_else(|| "file".to_owned());
        let mut form = MultipartForm::new()
            .file(
                "file",
                Some(filename),
                Some(options.media_type.as_str().to_owned()),
                data,
            )
            .field(
                "purpose",
                openai.purpose.as_deref().unwrap_or(DEFAULT_PURPOSE),
            );
        if let Some(expires) = &openai.expires_after {
            form = form
                .field(
                    "expires_after[anchor]",
                    expires.anchor.as_deref().unwrap_or("created_at"),
                )
                .field("expires_after[seconds]", expires.seconds.to_string());
        }
        let handlers = ResponseHandlers::new(
            json_response_handler::<OpenAiFileObject>(),
            failed_response_handler(),
        );
        let response = post_form(
            self.config.transport.as_ref(),
            self.config.url("/files"),
            self.config.headers(&options.headers)?,
            form,
            &handlers,
            options.cancellation,
        )
        .await?;
        Ok(self.to_result(&response.value, Some(options.media_type)))
    }

    fn supports_get_file_metadata(&self) -> bool {
        true
    }

    async fn get_file_metadata(
        &self,
        options: FileReferenceOptions,
    ) -> Result<FileMetadataResult, ProviderError> {
        let id = self.file_id(&options.file)?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<OpenAiFileObject>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            self.config.url(&format!("/files/{id}")),
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation,
        )
        .await?;
        Ok(self.to_result(&response.value, None))
    }

    fn supports_download_file(&self) -> bool {
        true
    }

    async fn download_file(
        &self,
        options: FileReferenceOptions,
    ) -> Result<DownloadFileResult, ProviderError> {
        let id = self.file_id(&options.file)?;
        let handlers =
            ResponseHandlers::new(binary_stream_response_handler(), failed_response_handler());
        let response = get(
            self.config.transport.as_ref(),
            self.config.url(&format!("/files/{id}/content")),
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation,
        )
        .await?;
        let media_type = response
            .response_headers
            .get_str("content-type")
            .map(|value| MediaType::new(value.split(';').next().unwrap_or(value).trim()));
        let content = response.value.map_err(ProviderError::other).boxed();
        Ok(DownloadFileResult {
            content,
            media_type,
            provider_metadata: None,
            warnings: Vec::new(),
        })
    }

    fn supports_delete_file(&self) -> bool {
        true
    }

    async fn delete_file(
        &self,
        options: FileReferenceOptions,
    ) -> Result<DeleteFileResult, ProviderError> {
        let id = self.file_id(&options.file)?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<DeleteResponse>(),
            failed_response_handler(),
        );
        let response = delete(
            self.config.transport.as_ref(),
            self.config.url(&format!("/files/{id}")),
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation,
        )
        .await?;
        Ok(DeleteFileResult {
            provider_reference: self.reference(&response.value.id),
            deleted: response.value.deleted,
            provider_metadata: None,
            warnings: Vec::new(),
        })
    }
}
