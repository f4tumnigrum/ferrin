//! Provider file storage: [`upload_file`], [`get_file_metadata`],
//! [`download_file`] and [`delete_file`].
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §7.

use std::fmt;
use std::future::IntoFuture;

use ferrin_provider_util::media_type::detect_media_type;
use ferrin_spec::BoxFuture;
use ferrin_spec::FilesRef;
use ferrin_spec::MediaType;
use ferrin_spec::ProviderReference;
use ferrin_spec::error::ProviderError;
pub use ferrin_spec::files::DeleteFileResult;
pub use ferrin_spec::files::DownloadFileResult;
pub use ferrin_spec::files::FileMetadataResult;
use ferrin_spec::files::FileReferenceOptions;
pub use ferrin_spec::files::UploadData;
use ferrin_spec::files::UploadFileOptions;
pub use ferrin_spec::files::UploadFileResult;
use tracing::Instrument;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
use crate::telemetry::ModelIdentity;
use crate::telemetry::spans;

/// Number of leading bytes inspected by [`is_likely_text`].
const TEXT_CHECK_LENGTH: usize = 512;

/// Uploads a file to the provider's storage. The media type is detected
/// from the data when not set: text data is `text/plain`, streams are
/// `application/octet-stream`.
#[must_use]
pub fn upload_file(files: impl Into<FilesRef>, data: impl Into<UploadData>) -> UploadFile {
    UploadFile {
        files: files.into(),
        data: data.into(),
        media_type: None,
        filename: None,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`upload_file`]; `.await` runs the upload.
pub struct UploadFile {
    files: FilesRef,
    data: UploadData,
    media_type: Option<MediaType>,
    filename: Option<String>,
    base: ModalityOptions,
}

impl fmt::Debug for UploadFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UploadFile")
            .field("files", &self.files)
            .field("data", &self.data)
            .field("media_type", &self.media_type)
            .field("filename", &self.filename)
            .field("base", &self.base)
            .finish()
    }
}

impl UploadFile {
    /// Sets the media type.
    #[must_use]
    pub fn media_type(mut self, media_type: impl Into<MediaType>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }

    /// Sets the file name.
    #[must_use]
    pub fn filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }
}

impl_modality_builder!(@no_retry UploadFile);

impl IntoFuture for UploadFile {
    type Output = Result<UploadFileResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let identity = service_identity(&self.files);
            let span = spans::modality_span("upload_file", &identity);
            let media_type = self
                .media_type
                .unwrap_or_else(|| default_media_type(&self.data));
            let files = self.files.clone();
            let base = self.base.clone();
            let data = self.data;
            let filename = self.filename;
            base.run(|base, token| {
                async move {
                    let result = files
                        .upload_file(UploadFileOptions {
                            data,
                            media_type,
                            filename,
                            headers: base.request_headers(),
                            provider_options: base.provider_options.clone(),
                            cancellation: token,
                        })
                        .await
                        .map_err(Error::from)?;
                    spans::log_warnings(&result.warnings, &identity);
                    Ok(result)
                }
                .instrument(span)
            })
            .await
        })
    }
}

/// Returns `true` when the first bytes contain no NUL and no control
/// characters other than tab, newline and carriage return.
pub(crate) fn is_likely_text(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(TEXT_CHECK_LENGTH)];
    !sample.is_empty()
        && sample
            .iter()
            .all(|byte| *byte >= 0x20 || matches!(byte, 0x09 | 0x0a | 0x0d))
}

fn default_media_type(data: &UploadData) -> MediaType {
    match data {
        UploadData::Text(_) => MediaType::new("text/plain"),
        UploadData::Bytes(bytes) => detect_media_type(bytes).unwrap_or_else(|| {
            if is_likely_text(bytes) {
                MediaType::new("text/plain")
            } else {
                MediaType::new("application/octet-stream")
            }
        }),
        #[allow(unreachable_patterns, reason = "UploadData is non-exhaustive")]
        _ => MediaType::new("application/octet-stream"),
    }
}

fn service_identity(files: &FilesRef) -> ModelIdentity {
    ModelIdentity::new(files.provider().clone(), "files")
}

macro_rules! file_operation {
    ($(#[$meta:meta])* $name:ident, $ty:ident, $result:ty, $supports:ident, $method:ident, $label:literal) => {
        $(#[$meta])*
        #[must_use]
        pub fn $name(files: impl Into<FilesRef>, file: ProviderReference) -> $ty {
            $ty {
                files: files.into(),
                file,
                base: ModalityOptions::default(),
            }
        }

        #[doc = concat!("Builder returned by [`", stringify!($name), "`]; `.await` runs the call.")]
        #[derive(Debug)]
        pub struct $ty {
            files: FilesRef,
            file: ProviderReference,
            base: ModalityOptions,
        }

        impl_modality_builder!(@no_retry $ty);

        impl IntoFuture for $ty {
            type Output = Result<$result, Error>;
            type IntoFuture = BoxFuture<'static, Self::Output>;

            fn into_future(self) -> Self::IntoFuture {
                Box::pin(async move {
                    let identity = service_identity(&self.files);
                    let span = spans::modality_span($label, &identity);
                    if !self.files.$supports() {
                        return Err(Error::from(ProviderError::unsupported($label)));
                    }
                    let files = self.files.clone();
                    let base = self.base.clone();
                    let file = self.file;
                    base.run(|base, token| {
                        async move {
                            files
                                .$method(FileReferenceOptions {
                                    file,
                                    headers: base.request_headers(),
                                    provider_options: base.provider_options.clone(),
                                    cancellation: token,
                                })
                                .await
                                .map_err(Error::from)
                        }
                        .instrument(span)
                    })
                    .await
                })
            }
        }
    };
}

file_operation!(
    /// Fetches the metadata of an uploaded file. Fails with an unsupported
    /// functionality error when the provider does not implement it.
    get_file_metadata,
    GetFileMetadata,
    FileMetadataResult,
    supports_get_file_metadata,
    get_file_metadata,
    "get_file_metadata"
);

file_operation!(
    /// Downloads an uploaded file. Fails with an unsupported functionality
    /// error when the provider does not implement it.
    download_file,
    DownloadFile,
    DownloadFileResult,
    supports_download_file,
    download_file,
    "download_file"
);

file_operation!(
    /// Deletes an uploaded file. Fails with an unsupported functionality
    /// error when the provider does not implement it.
    delete_file,
    DeleteFile,
    DeleteFileResult,
    supports_delete_file,
    delete_file,
    "delete_file"
);
