//! Object-safe files service.

use super::BoxFuture;
use super::ServiceRef;
use super::model_ref::ref_conversions;
use crate::error::ProviderError;
use crate::files::DeleteFileResult;
use crate::files::DownloadFileResult;
use crate::files::FileMetadataResult;
use crate::files::FileReferenceOptions;
use crate::files::Files;
use crate::files::UploadFileOptions;
use crate::files::UploadFileResult;
use crate::shared::ProviderId;

/// Object-safe counterpart of [`Files`].
pub trait DynFiles: Send + Sync + 'static {
    /// See [`Files::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`Files::upload_file`].
    fn upload_file(
        &self,
        options: UploadFileOptions,
    ) -> BoxFuture<'_, Result<UploadFileResult, ProviderError>>;
    /// See [`Files::supports_get_file_metadata`].
    fn supports_get_file_metadata(&self) -> bool;
    /// See [`Files::get_file_metadata`].
    fn get_file_metadata(
        &self,
        options: FileReferenceOptions,
    ) -> BoxFuture<'_, Result<FileMetadataResult, ProviderError>>;
    /// See [`Files::supports_download_file`].
    fn supports_download_file(&self) -> bool;
    /// See [`Files::download_file`].
    fn download_file(
        &self,
        options: FileReferenceOptions,
    ) -> BoxFuture<'_, Result<DownloadFileResult, ProviderError>>;
    /// See [`Files::supports_delete_file`].
    fn supports_delete_file(&self) -> bool;
    /// See [`Files::delete_file`].
    fn delete_file(
        &self,
        options: FileReferenceOptions,
    ) -> BoxFuture<'_, Result<DeleteFileResult, ProviderError>>;
}

impl<T: Files> DynFiles for T {
    fn provider(&self) -> &ProviderId {
        Files::provider(self)
    }

    fn upload_file(
        &self,
        options: UploadFileOptions,
    ) -> BoxFuture<'_, Result<UploadFileResult, ProviderError>> {
        Box::pin(Files::upload_file(self, options))
    }

    fn supports_get_file_metadata(&self) -> bool {
        Files::supports_get_file_metadata(self)
    }

    fn get_file_metadata(
        &self,
        options: FileReferenceOptions,
    ) -> BoxFuture<'_, Result<FileMetadataResult, ProviderError>> {
        Box::pin(Files::get_file_metadata(self, options))
    }

    fn supports_download_file(&self) -> bool {
        Files::supports_download_file(self)
    }

    fn download_file(
        &self,
        options: FileReferenceOptions,
    ) -> BoxFuture<'_, Result<DownloadFileResult, ProviderError>> {
        Box::pin(Files::download_file(self, options))
    }

    fn supports_delete_file(&self) -> bool {
        Files::supports_delete_file(self)
    }

    fn delete_file(
        &self,
        options: FileReferenceOptions,
    ) -> BoxFuture<'_, Result<DeleteFileResult, ProviderError>> {
        Box::pin(Files::delete_file(self, options))
    }
}

/// Shared reference to a files service.
pub type FilesRef = ServiceRef<dyn DynFiles>;

ref_conversions!(FilesRef, Files, DynFiles);
