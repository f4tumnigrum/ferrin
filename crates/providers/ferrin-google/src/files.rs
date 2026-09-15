//! Files API: resumable upload with processing poll, metadata and delete.

use std::time::Duration;
use std::time::Instant;

use bytes::Bytes;
use bytes::BytesMut;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::RequestBody;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::delete;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::http::send;
use ferrin_provider_util::http::text_response_handler;
use ferrin_provider_util::secure_url::validate_url;
use ferrin_spec::Headers;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderReference;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::files::DeleteFileResult;
use ferrin_spec::files::FileMetadataResult;
use ferrin_spec::files::FileReferenceOptions;
use ferrin_spec::files::Files;
use ferrin_spec::files::UploadData;
use ferrin_spec::files::UploadFileOptions;
use ferrin_spec::files::UploadFileResult;
use futures_util::StreamExt;
use futures_util::future::Either;
use futures_util::future::select;
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::api_types::deserialize_count;
use crate::config::CANONICAL_OPTIONS_KEY;
use crate::config::SharedConfig;
use crate::config::UPLOAD_PATH;
use crate::convert_prompt::resolve_reference;
use crate::error::failed_response_handler;
use crate::options::part_options;
use crate::options::read_options;
use crate::output::OutputMapper;

/// Default interval between processing-state polls.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 2_000;

/// Default time after which a still-processing upload fails.
pub const DEFAULT_POLL_TIMEOUT_MS: u64 = 300_000;

/// Provider options of uploads (`provider_options["google"]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFilesOptions {
    /// Display name stored with the file (defaults to `filename`).
    #[serde(default)]
    pub display_name: Option<String>,
    /// Poll interval in milliseconds while the file is `PROCESSING`.
    #[serde(default)]
    pub poll_interval_ms: Option<u64>,
    /// Poll timeout in milliseconds.
    #[serde(default)]
    pub poll_timeout_ms: Option<u64>,
}

/// A file resource.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFile {
    /// Resource name (`files/abc-123`).
    pub name: String,
    /// Display name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Media type.
    #[serde(default)]
    pub mime_type: Option<String>,
    /// Size in bytes.
    #[serde(default, deserialize_with = "deserialize_count")]
    pub size_bytes: Option<u64>,
    /// Creation time (RFC 3339).
    #[serde(default)]
    pub create_time: Option<String>,
    /// Update time (RFC 3339).
    #[serde(default)]
    pub update_time: Option<String>,
    /// Expiration time (RFC 3339).
    #[serde(default)]
    pub expiration_time: Option<String>,
    /// SHA-256 hash (base64).
    #[serde(default)]
    pub sha256_hash: Option<String>,
    /// URI usable as `fileData.fileUri`.
    #[serde(default)]
    pub uri: Option<String>,
    /// `PROCESSING`, `ACTIVE` or `FAILED`.
    #[serde(default)]
    pub state: Option<String>,
}

/// Upload responses wrap the file in `{file}`; `files.get` returns it bare.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FileResponse {
    Envelope { file: GoogleFile },
    Bare(GoogleFile),
}

impl FileResponse {
    fn into_file(self) -> GoogleFile {
        match self {
            Self::Envelope { file } | Self::Bare(file) => file,
        }
    }
}

/// A byte upload through the resumable protocol.
#[derive(Debug, Clone)]
pub struct UploadRequest {
    /// Content.
    pub data: Bytes,
    /// Media type sent as `X-Goog-Upload-Header-Content-Type`.
    pub media_type: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Poll interval while `PROCESSING`.
    pub poll_interval: Duration,
    /// Poll timeout.
    pub poll_timeout: Duration,
    /// Additional request headers.
    pub headers: Headers,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl UploadRequest {
    /// Creates a request with the default poll settings.
    #[must_use]
    pub fn new(data: Bytes, media_type: impl Into<String>) -> Self {
        Self {
            data,
            media_type: media_type.into(),
            display_name: None,
            poll_interval: Duration::from_millis(DEFAULT_POLL_INTERVAL_MS),
            poll_timeout: Duration::from_millis(DEFAULT_POLL_TIMEOUT_MS),
            headers: Headers::new(),
            cancellation: CancellationToken::new(),
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
        #[allow(unreachable_patterns, reason = "UploadData is non-exhaustive")]
        _ => Err(ProviderError::unsupported("upload data type")),
    }
}

fn parse_time(value: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    value
        .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
        .map(|time| time.with_timezone(&chrono::Utc))
}

/// Normalizes a provider reference (a `files/...` name, a bare id or the
/// full `uri`) to the resource name.
fn file_name(reference: &str) -> String {
    if let Some((_, rest)) = reference.rsplit_once("/files/") {
        return format!("files/{rest}");
    }
    if reference.starts_with("files/") {
        return reference.to_owned();
    }
    format!("files/{reference}")
}

/// Files service backed by the Files API.
#[derive(Debug, Clone)]
pub struct GoogleFiles {
    config: SharedConfig,
    provider: ProviderId,
}

impl GoogleFiles {
    /// Creates the service.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: ProviderId::new(config.name.clone()),
            config,
        }
    }

    fn reference(&self, file: &GoogleFile) -> ProviderReference {
        let value = file.uri.clone().unwrap_or_else(|| file.name.clone());
        let mut reference = ProviderReference::new();
        reference.insert(CANONICAL_OPTIONS_KEY.to_owned(), value.clone());
        reference.insert(self.config.name.clone(), value);
        reference
    }

    /// Maps a file resource to the specification result.
    #[must_use]
    pub fn to_result(&self, file: &GoogleFile) -> UploadFileResult {
        let mut meta = JsonObject::new();
        let string =
            |value: &Option<String>| value.clone().map_or(JsonValue::Null, JsonValue::from);
        meta.insert("name".to_owned(), JsonValue::from(file.name.clone()));
        meta.insert("displayName".to_owned(), string(&file.display_name));
        meta.insert("mimeType".to_owned(), string(&file.mime_type));
        meta.insert(
            "sizeBytes".to_owned(),
            file.size_bytes.map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert("state".to_owned(), string(&file.state));
        meta.insert("uri".to_owned(), string(&file.uri));
        meta.insert("createTime".to_owned(), string(&file.create_time));
        meta.insert("updateTime".to_owned(), string(&file.update_time));
        meta.insert("expirationTime".to_owned(), string(&file.expiration_time));
        meta.insert("sha256Hash".to_owned(), string(&file.sha256_hash));
        let mapper = OutputMapper::new(self.config.clone(), Default::default());
        UploadFileResult {
            provider_reference: self.reference(file),
            media_type: file.mime_type.as_deref().map(MediaType::new),
            filename: file.display_name.clone(),
            byte_size: file.size_bytes,
            created_at: parse_time(file.create_time.as_deref()),
            expires_at: parse_time(file.expiration_time.as_deref()),
            provider_metadata: Some(mapper.metadata(meta)),
            warnings: Vec::new(),
        }
    }

    /// Fetches the resource `name` (`files/...`).
    ///
    /// # Errors
    ///
    /// Returns the API error of the request.
    pub async fn fetch_file(
        &self,
        name: &str,
        headers: &Headers,
        cancellation: CancellationToken,
    ) -> Result<GoogleFile, ProviderError> {
        let handlers = ResponseHandlers::new(
            json_response_handler::<FileResponse>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            self.config.url(name),
            self.config.headers(headers)?,
            &handlers,
            cancellation,
        )
        .await?;
        Ok(response.value.into_file())
    }

    /// Uploads bytes through the resumable protocol and waits until the file
    /// leaves the `PROCESSING` state.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::InvalidResponseData`] when the upload session
    /// returns no upload URL, [`ProviderError::ApiCall`] when processing
    /// fails or times out, and [`ProviderError::Cancelled`] when the token
    /// fires while polling.
    #[tracing::instrument(skip_all, fields(media_type = %request.media_type, bytes = request.data.len()))]
    pub async fn upload_bytes(&self, request: UploadRequest) -> Result<GoogleFile, ProviderError> {
        let start_url = self.config.origin_url(UPLOAD_PATH);
        let start_headers = self
            .config
            .headers(&request.headers)?
            .with("x-goog-upload-protocol", "resumable")
            .with("x-goog-upload-command", "start")
            .with(
                "x-goog-upload-header-content-length",
                &request.data.len().to_string(),
            )
            .with("x-goog-upload-header-content-type", &request.media_type);
        let mut file = JsonObject::new();
        if let Some(name) = &request.display_name {
            file.insert("display_name".to_owned(), JsonValue::from(name.as_str()));
        }
        let start_handlers =
            ResponseHandlers::new(text_response_handler(), failed_response_handler());
        let started = post_json(
            self.config.transport.as_ref(),
            start_url,
            start_headers,
            &json!({"file": file}),
            &start_handlers,
            request.cancellation.clone(),
        )
        .await?;
        let upload_url = started
            .response_headers
            .get_str("x-goog-upload-url")
            .and_then(|value| Url::parse(value).ok())
            .ok_or_else(|| {
                ProviderError::InvalidResponseData(Box::new(InvalidResponseDataError::new(
                    "google did not return a resumable upload URL",
                    JsonValue::Null,
                )))
            })?;
        let validated = validate_url(&upload_url, &self.config.url_policy)
            .await
            .map_err(|error| {
                InvalidResponseDataError::new(
                    format!("google returned an unsafe upload URL: {error}"),
                    JsonValue::Null,
                )
            })?;
        let mut finalize_headers = if upload_url.origin() == self.config.base_url.origin()
            || self.config.url_policy.is_credentialed(&upload_url)
        {
            self.config.unauthenticated_headers(&request.headers)
        } else {
            Headers::new().with_user_agent_suffix([crate::config::USER_AGENT])
        };
        finalize_headers.remove(crate::config::API_KEY_HEADER);
        let finalize_headers = finalize_headers
            .with("x-goog-upload-offset", "0")
            .with("x-goog-upload-command", "upload, finalize");
        let handlers = ResponseHandlers::new(
            json_response_handler::<FileResponse>()
                .with_max_bytes(self.config.url_policy.max_body_bytes),
            failed_response_handler().with_max_bytes(self.config.url_policy.max_body_bytes),
        );
        let finalize = HttpRequest::post(upload_url.clone())
            .with_headers(finalize_headers)
            .with_body(RequestBody::Bytes {
                content_type: request.media_type.clone(),
                data: request.data,
            })
            .with_cancellation(request.cancellation.clone())
            .with_pinned_addresses(validated.addresses);
        let uploaded = send(self.config.transport.as_ref(), finalize, None, &handlers).await?;
        let mut file = uploaded.value.into_file();
        let started_at = Instant::now();
        while file.state.as_deref() == Some("PROCESSING") {
            if started_at.elapsed() > request.poll_timeout {
                return Err(ProviderError::ApiCall(Box::new(ApiCallError::new(
                    format!(
                        "file processing timed out after {}ms",
                        request.poll_timeout.as_millis()
                    ),
                    upload_url,
                ))));
            }
            let sleep = Box::pin(tokio::time::sleep(request.poll_interval));
            let cancelled = Box::pin(request.cancellation.cancelled());
            if let Either::Right(_) = select(sleep, cancelled).await {
                return Err(ProviderError::Cancelled);
            }
            file = self
                .fetch_file(&file.name, &request.headers, request.cancellation.clone())
                .await?;
        }
        if file.state.as_deref() == Some("FAILED") {
            return Err(ProviderError::ApiCall(Box::new(ApiCallError::new(
                format!("file processing failed for {}", file.name),
                upload_url,
            ))));
        }
        Ok(file)
    }
}

impl Files for GoogleFiles {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    async fn upload_file(
        &self,
        options: UploadFileOptions,
    ) -> Result<UploadFileResult, ProviderError> {
        let google: GoogleFilesOptions =
            read_options(part_options(&self.config, Some(&options.provider_options)))
                .unwrap_or_default();
        let data = collect(options.data).await?;
        let mut request = UploadRequest::new(data, options.media_type.as_str());
        request.display_name = google.display_name.or(options.filename);
        if let Some(interval) = google.poll_interval_ms {
            request.poll_interval = Duration::from_millis(interval);
        }
        if let Some(timeout) = google.poll_timeout_ms {
            request.poll_timeout = Duration::from_millis(timeout);
        }
        request.headers = options.headers;
        request.cancellation = options.cancellation;
        let file = self.upload_bytes(request).await?;
        Ok(self.to_result(&file))
    }

    fn supports_get_file_metadata(&self) -> bool {
        true
    }

    async fn get_file_metadata(
        &self,
        options: FileReferenceOptions,
    ) -> Result<FileMetadataResult, ProviderError> {
        let name = file_name(resolve_reference(&self.config, &options.file)?);
        let file = self
            .fetch_file(&name, &options.headers, options.cancellation)
            .await?;
        Ok(self.to_result(&file))
    }

    fn supports_delete_file(&self) -> bool {
        true
    }

    async fn delete_file(
        &self,
        options: FileReferenceOptions,
    ) -> Result<DeleteFileResult, ProviderError> {
        let name = file_name(resolve_reference(&self.config, &options.file)?);
        let handlers = ResponseHandlers::new(text_response_handler(), failed_response_handler());
        delete(
            self.config.transport.as_ref(),
            self.config.url(&name),
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation,
        )
        .await?;
        Ok(DeleteFileResult {
            provider_reference: options.file,
            deleted: true,
            provider_metadata: None,
            warnings: Vec::new(),
        })
    }
}
