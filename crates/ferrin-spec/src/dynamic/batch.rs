//! Object-safe batch service.

use super::BoxFuture;
use super::ServiceRef;
use super::model_ref::ref_conversions;
use crate::batch::Batch;
use crate::batch::BatchCancelResult;
use crate::batch::BatchListOptions;
use crate::batch::BatchListResult;
use crate::batch::BatchOperationOptions;
use crate::batch::BatchResultStream;
use crate::batch::BatchStartOptions;
use crate::batch::BatchStartResult;
use crate::batch::BatchStatus;
use crate::error::ProviderError;
use crate::language_model::SupportedUrls;
use crate::shared::ProviderId;

/// Object-safe counterpart of [`Batch`].
pub trait DynBatch: Send + Sync + 'static {
    /// See [`Batch::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`Batch::supported_urls`].
    fn supported_urls(&self) -> BoxFuture<'_, SupportedUrls>;
    /// See [`Batch::do_start_batch`].
    fn do_start_batch(
        &self,
        options: BatchStartOptions,
    ) -> BoxFuture<'_, Result<BatchStartResult, ProviderError>>;
    /// See [`Batch::do_get_batch_status`].
    fn do_get_batch_status(
        &self,
        options: BatchOperationOptions,
    ) -> BoxFuture<'_, Result<BatchStatus, ProviderError>>;
    /// See [`Batch::do_get_batch_results`].
    fn do_get_batch_results(
        &self,
        options: BatchOperationOptions,
    ) -> BoxFuture<'_, Result<BatchResultStream, ProviderError>>;
    /// See [`Batch::supports_cancel_batch`].
    fn supports_cancel_batch(&self) -> bool;
    /// See [`Batch::do_cancel_batch`].
    fn do_cancel_batch(
        &self,
        options: BatchOperationOptions,
    ) -> BoxFuture<'_, Result<BatchCancelResult, ProviderError>>;
    /// See [`Batch::supports_list_batches`].
    fn supports_list_batches(&self) -> bool;
    /// See [`Batch::do_list_batches`].
    fn do_list_batches(
        &self,
        options: BatchListOptions,
    ) -> BoxFuture<'_, Result<BatchListResult, ProviderError>>;
}

impl<T: Batch> DynBatch for T {
    fn provider(&self) -> &ProviderId {
        Batch::provider(self)
    }

    fn supported_urls(&self) -> BoxFuture<'_, SupportedUrls> {
        Box::pin(Batch::supported_urls(self))
    }

    fn do_start_batch(
        &self,
        options: BatchStartOptions,
    ) -> BoxFuture<'_, Result<BatchStartResult, ProviderError>> {
        Box::pin(Batch::do_start_batch(self, options))
    }

    fn do_get_batch_status(
        &self,
        options: BatchOperationOptions,
    ) -> BoxFuture<'_, Result<BatchStatus, ProviderError>> {
        Box::pin(Batch::do_get_batch_status(self, options))
    }

    fn do_get_batch_results(
        &self,
        options: BatchOperationOptions,
    ) -> BoxFuture<'_, Result<BatchResultStream, ProviderError>> {
        Box::pin(Batch::do_get_batch_results(self, options))
    }

    fn supports_cancel_batch(&self) -> bool {
        Batch::supports_cancel_batch(self)
    }

    fn do_cancel_batch(
        &self,
        options: BatchOperationOptions,
    ) -> BoxFuture<'_, Result<BatchCancelResult, ProviderError>> {
        Box::pin(Batch::do_cancel_batch(self, options))
    }

    fn supports_list_batches(&self) -> bool {
        Batch::supports_list_batches(self)
    }

    fn do_list_batches(
        &self,
        options: BatchListOptions,
    ) -> BoxFuture<'_, Result<BatchListResult, ProviderError>> {
        Box::pin(Batch::do_list_batches(self, options))
    }
}

/// Shared reference to a batch service.
pub type BatchRef = ServiceRef<dyn DynBatch>;

ref_conversions!(BatchRef, Batch, DynBatch);
