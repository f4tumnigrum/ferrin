//! Files API (`<name>.files`): uploads only.

use std::collections::BTreeSet;

use bytes::Bytes;
use chrono::DateTime;
use chrono::Utc;
use ferrin_provider_util::MultipartForm;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::TransportError;
use ferrin_provider_util::http::TransportErrorKind;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_form;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderReference;
use ferrin_spec::error::ProviderError;
use ferrin_spec::files::Files;
use ferrin_spec::files::UploadData;
use ferrin_spec::files::UploadFileOptions;
use ferrin_spec::files::UploadFileResult;
use futures_util::StreamExt;
use futures_util::TryStreamExt;
use serde::Deserialize;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::output::anthropic_metadata;

/// Beta flag of the Files API.
pub const FILES_BETA: &str = "files-api-2025-04-14";

/// A file object returned by the API.
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicFileObject {
    /// File id.
    pub id: String,
    /// File name.
    #[serde(default)]
    pub filename: Option<String>,
    /// Media type.
    #[serde(default)]
    pub mime_type: Option<String>,
    /// Size in bytes.
    #[serde(default)]
    pub size_bytes: Option<u64>,
    /// Creation time (RFC 3339).
    #[serde(default)]
    pub created_at: Option<String>,
    /// Whether the file can be downloaded.
    #[serde(default)]
    pub downloadable: Option<bool>,
}

/// Files service backed by `POST /files`.
#[derive(Debug, Clone)]
pub struct AnthropicFiles {
    config: SharedConfig,
    provider: ProviderId,
}

/// Parses an RFC 3339 timestamp.
#[must_use]
pub(crate) fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn file_form(data: UploadData, filename: Option<String>, media_type: String) -> MultipartForm {
    let form = MultipartForm::new();
    match data {
        UploadData::Bytes(data) => form.file("file", filename, Some(media_type), data),
        UploadData::Text(text) => form.file("file", filename, Some(media_type), Bytes::from(text)),
        UploadData::Stream(stream) => form.file_stream(
            "file",
            filename,
            Some(media_type),
            stream
                .map_err(|error| {
                    if matches!(error, ProviderError::Cancelled) {
                        TransportError::cancelled()
                    } else {
                        TransportError::new(TransportErrorKind::Body, "file upload stream failed")
                            .with_cause(error)
                    }
                })
                .boxed(),
        ),
    }
}

impl AnthropicFiles {
    /// Creates the service.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: config.provider_id("files"),
            config,
        }
    }

    fn to_result(
        &self,
        file: &AnthropicFileObject,
        media_type: MediaType,
        filename: Option<String>,
    ) -> UploadFileResult {
        let mut reference = ProviderReference::new();
        reference.insert(self.config.name.clone(), file.id.clone());
        let mut meta = JsonObject::new();
        meta.insert(
            "filename".to_owned(),
            file.filename
                .clone()
                .map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "mimeType".to_owned(),
            file.mime_type
                .clone()
                .map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "sizeBytes".to_owned(),
            file.size_bytes.map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "createdAt".to_owned(),
            file.created_at
                .clone()
                .map_or(JsonValue::Null, JsonValue::from),
        );
        if let Some(downloadable) = file.downloadable {
            meta.insert("downloadable".to_owned(), JsonValue::Bool(downloadable));
        }
        UploadFileResult {
            provider_reference: reference,
            media_type: Some(file.mime_type.clone().map_or(media_type, MediaType::new)),
            filename: file.filename.clone().or(filename),
            byte_size: file.size_bytes,
            created_at: parse_timestamp(file.created_at.as_deref()),
            expires_at: None,
            provider_metadata: Some(anthropic_metadata(meta)),
            warnings: Vec::new(),
        }
    }
}

impl Files for AnthropicFiles {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    #[tracing::instrument(skip_all)]
    async fn upload_file(
        &self,
        options: UploadFileOptions,
    ) -> Result<UploadFileResult, ProviderError> {
        let form = file_form(
            options.data,
            Some(
                options
                    .filename
                    .clone()
                    .unwrap_or_else(|| "blob".to_owned()),
            ),
            options.media_type.as_str().to_owned(),
        );
        let betas = BTreeSet::from([FILES_BETA.to_owned()]);
        let handlers = ResponseHandlers::new(
            json_response_handler::<AnthropicFileObject>(),
            failed_response_handler(),
        );
        let response = post_form(
            self.config.transport.as_ref(),
            self.config.url("/files"),
            self.config.headers(&options.headers, &betas)?,
            form,
            &handlers,
            options.cancellation,
        )
        .await?;
        Ok(self.to_result(&response.value, options.media_type, options.filename))
    }
}
