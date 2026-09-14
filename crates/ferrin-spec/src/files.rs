//! Provider file storage interface.

use std::future::Future;

use bytes::Bytes;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::dynamic::BoxStream;
use crate::error::ProviderError;
use crate::shared::Headers;
use crate::shared::MediaType;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::ProviderReference;
use crate::shared::Warning;

/// Upload, inspect, download and delete files stored at the provider.
///
/// Only [`upload_file`](Self::upload_file) is required; the other operations
/// are gated by `supports_*` queries and default to an
/// `UnsupportedFunctionality` error.
pub trait Files: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Uploads a file and returns its provider reference.
    fn upload_file(
        &self,
        options: UploadFileOptions,
    ) -> impl Future<Output = Result<UploadFileResult, ProviderError>> + Send;

    /// Whether [`get_file_metadata`](Self::get_file_metadata) is implemented.
    fn supports_get_file_metadata(&self) -> bool {
        false
    }

    /// Fetches metadata of an uploaded file.
    fn get_file_metadata(
        &self,
        options: FileReferenceOptions,
    ) -> impl Future<Output = Result<FileMetadataResult, ProviderError>> + Send {
        let _ = options;
        std::future::ready(Err(ProviderError::unsupported("get_file_metadata")))
    }

    /// Whether [`download_file`](Self::download_file) is implemented.
    fn supports_download_file(&self) -> bool {
        false
    }

    /// Downloads an uploaded file.
    fn download_file(
        &self,
        options: FileReferenceOptions,
    ) -> impl Future<Output = Result<DownloadFileResult, ProviderError>> + Send {
        let _ = options;
        std::future::ready(Err(ProviderError::unsupported("download_file")))
    }

    /// Whether [`delete_file`](Self::delete_file) is implemented.
    fn supports_delete_file(&self) -> bool {
        false
    }

    /// Deletes an uploaded file.
    fn delete_file(
        &self,
        options: FileReferenceOptions,
    ) -> impl Future<Output = Result<DeleteFileResult, ProviderError>> + Send {
        let _ = options;
        std::future::ready(Err(ProviderError::unsupported("delete_file")))
    }
}

/// Data to upload.
pub enum UploadData {
    /// In-memory bytes.
    Bytes(Bytes),
    /// UTF-8 text.
    Text(String),
    /// A stream of chunks.
    Stream(BoxStream<'static, Result<Bytes, ProviderError>>),
}

impl std::fmt::Debug for UploadData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(bytes) => f.debug_tuple("Bytes").field(&bytes.len()).finish(),
            Self::Text(text) => f.debug_tuple("Text").field(&text.len()).finish(),
            Self::Stream(_) => f.write_str("Stream(<stream>)"),
        }
    }
}

impl From<Bytes> for UploadData {
    fn from(bytes: Bytes) -> Self {
        Self::Bytes(bytes)
    }
}

impl From<Vec<u8>> for UploadData {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes(Bytes::from(bytes))
    }
}

impl From<String> for UploadData {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

/// Options for uploading a file.
#[derive(Debug)]
pub struct UploadFileOptions {
    /// The data.
    pub data: UploadData,
    /// Media type of the data.
    pub media_type: MediaType,
    /// File name.
    pub filename: Option<String>,
    /// Additional request headers.
    pub headers: Headers,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl UploadFileOptions {
    /// Creates options for `data` of `media_type`.
    #[must_use]
    pub fn new(data: impl Into<UploadData>, media_type: impl Into<MediaType>) -> Self {
        Self {
            data: data.into(),
            media_type: media_type.into(),
            filename: None,
            headers: Headers::new(),
            provider_options: ProviderOptions::new(),
            cancellation: CancellationToken::new(),
        }
    }
}

/// Options for operations on an existing file.
#[derive(Debug, Clone)]
pub struct FileReferenceOptions {
    /// Reference of the file.
    pub file: ProviderReference,
    /// Additional request headers.
    pub headers: Headers,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl FileReferenceOptions {
    /// Creates options for `file`.
    #[must_use]
    pub fn new(file: ProviderReference) -> Self {
        Self {
            file,
            headers: Headers::new(),
            provider_options: ProviderOptions::new(),
            cancellation: CancellationToken::new(),
        }
    }
}

/// Result of an upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadFileResult {
    /// Reference to the stored file.
    pub provider_reference: ProviderReference,
    /// Media type as stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<MediaType>,
    /// File name as stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_size: Option<u64>,
    /// Creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    /// Expiry time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
}

/// Metadata of a stored file.
pub type FileMetadataResult = UploadFileResult;

/// Result of a download.
pub struct DownloadFileResult {
    /// File content.
    pub content: BoxStream<'static, Result<Bytes, ProviderError>>,
    /// Media type, if known.
    pub media_type: Option<MediaType>,
    /// Provider-specific metadata.
    pub provider_metadata: Option<ProviderMetadata>,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

impl std::fmt::Debug for DownloadFileResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadFileResult")
            .field("content", &"<stream>")
            .field("media_type", &self.media_type)
            .field("provider_metadata", &self.provider_metadata)
            .field("warnings", &self.warnings)
            .finish()
    }
}

/// Result of a delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteFileResult {
    /// Reference of the deleted file.
    pub provider_reference: ProviderReference,
    /// Whether the file was deleted.
    pub deleted: bool,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
}
