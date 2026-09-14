//! `get_batch_status`, `get_batch_results`, `cancel_batch` and `list_batches`.

use std::future::IntoFuture;
use std::sync::Arc;

pub(super) use ferrin_spec::BatchId;
use ferrin_spec::BatchRef;
use ferrin_spec::BoxFuture;
use ferrin_spec::batch::BatchCancelResult;
use ferrin_spec::batch::BatchListOptions;
use ferrin_spec::batch::BatchListResult;
use ferrin_spec::batch::BatchOperationOptions;
use ferrin_spec::batch::BatchStatus;
use ferrin_spec::error::ProviderError;
use ferrin_tool::ToolSet;
use futures_util::StreamExt;
use tracing::Instrument;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
use crate::retry::retry;

use super::result::BatchResults;
use super::result::convert_item;
use super::service_identity;
use crate::telemetry::spans;

macro_rules! batch_operation {
    ($(#[$meta:meta])* $name:ident, $ty:ident) => {
        $(#[$meta])*
        #[must_use]
        pub fn $name(batch: impl Into<BatchRef>, batch_id: impl Into<BatchId>) -> $ty {
            $ty {
                batch: batch.into(),
                batch_id: batch_id.into(),
                base: ModalityOptions::default(),
            }
        }
    };
}

batch_operation!(
    /// Fetches the normalized status of a batch.
    get_batch_status,
    GetBatchStatus
);

/// Builder returned by [`get_batch_status`]; `.await` runs the call.
#[derive(Debug)]
pub struct GetBatchStatus {
    batch: BatchRef,
    batch_id: BatchId,
    base: ModalityOptions,
}

impl_modality_builder!(GetBatchStatus);

impl IntoFuture for GetBatchStatus {
    type Output = Result<BatchStatus, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let identity = service_identity(&self.batch);
            let span = spans::modality_span("get_batch_status", &identity);
            let batch = self.batch.clone();
            let base = self.base.clone();
            let batch_id = self.batch_id;
            base.run(|base, token| {
                async move {
                    let headers = base.request_headers();
                    retry(&base.retry_policy, &token, |_| {
                        let options = BatchOperationOptions {
                            batch_id: batch_id.clone(),
                            provider_options: base.provider_options.clone(),
                            headers: headers.clone(),
                            cancellation: token.child_token(),
                        };
                        let batch = &batch;
                        async move {
                            batch
                                .do_get_batch_status(options)
                                .await
                                .map_err(Error::from)
                        }
                    })
                    .await
                }
                .instrument(span)
            })
            .await
        })
    }
}

/// Streams the results of a finished batch.
#[must_use]
pub fn get_batch_results(
    batch: impl Into<BatchRef>,
    batch_id: impl Into<BatchId>,
) -> GetBatchResults {
    GetBatchResults {
        batch: batch.into(),
        batch_id: batch_id.into(),
        tools: ToolSet::new(),
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`get_batch_results`]; `.await` opens the stream.
#[derive(Debug)]
pub struct GetBatchResults {
    batch: BatchRef,
    batch_id: BatchId,
    tools: ToolSet,
    base: ModalityOptions,
}

impl GetBatchResults {
    /// Tools used to parse and validate tool calls in text results.
    #[must_use]
    pub fn tools(mut self, tools: ToolSet) -> Self {
        self.tools = tools;
        self
    }
}

impl_modality_builder!(GetBatchResults);

impl IntoFuture for GetBatchResults {
    type Output = Result<BatchResults, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let identity = service_identity(&self.batch);
            let span = spans::modality_span("get_batch_results", &identity);
            let batch = self.batch.clone();
            let base = self.base.clone();
            let batch_id = self.batch_id;
            let tools = Arc::new(self.tools);
            let stream = base
                .run(|base, token| {
                    async move {
                        let headers = base.request_headers();
                        retry(&base.retry_policy, &token, |_| {
                            let options = BatchOperationOptions {
                                batch_id: batch_id.clone(),
                                provider_options: base.provider_options.clone(),
                                headers: headers.clone(),
                                cancellation: token.child_token(),
                            };
                            let batch = &batch;
                            async move {
                                batch
                                    .do_get_batch_results(options)
                                    .await
                                    .map_err(Error::from)
                            }
                        })
                        .await
                    }
                    .instrument(span)
                })
                .await?;
            let converted = stream.then(move |item| {
                let tools = Arc::clone(&tools);
                async move {
                    match item {
                        Ok(item) => Ok(convert_item(item, &tools).await),
                        Err(error) => Err(Error::from(error)),
                    }
                }
            });
            Ok(Box::pin(converted) as BatchResults)
        })
    }
}

batch_operation!(
    /// Cancels a batch. Fails with an unsupported functionality error when
    /// the provider does not implement cancellation.
    cancel_batch,
    CancelBatch
);

/// Builder returned by [`cancel_batch`]; `.await` runs the call.
#[derive(Debug)]
pub struct CancelBatch {
    batch: BatchRef,
    batch_id: BatchId,
    base: ModalityOptions,
}

impl_modality_builder!(@no_retry CancelBatch);

impl IntoFuture for CancelBatch {
    type Output = Result<BatchCancelResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let identity = service_identity(&self.batch);
            let span = spans::modality_span("cancel_batch", &identity);
            if !self.batch.supports_cancel_batch() {
                return Err(Error::from(ProviderError::unsupported(
                    "batch cancellation",
                )));
            }
            let batch = self.batch.clone();
            let base = self.base.clone();
            let batch_id = self.batch_id;
            base.run(|base, token| {
                async move {
                    batch
                        .do_cancel_batch(BatchOperationOptions {
                            batch_id,
                            provider_options: base.provider_options.clone(),
                            headers: base.request_headers(),
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

/// Lists batches. Fails with an unsupported functionality error when the
/// provider does not implement listing.
#[must_use]
pub fn list_batches(batch: impl Into<BatchRef>) -> ListBatches {
    ListBatches {
        batch: batch.into(),
        limit: None,
        cursor: None,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`list_batches`]; `.await` runs the call.
#[derive(Debug)]
pub struct ListBatches {
    batch: BatchRef,
    limit: Option<usize>,
    cursor: Option<String>,
    base: ModalityOptions,
}

impl ListBatches {
    /// Page size.
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Cursor of the next page.
    #[must_use]
    pub fn cursor(mut self, cursor: impl Into<String>) -> Self {
        self.cursor = Some(cursor.into());
        self
    }
}

impl_modality_builder!(ListBatches);

impl IntoFuture for ListBatches {
    type Output = Result<BatchListResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let identity = service_identity(&self.batch);
            let span = spans::modality_span("list_batches", &identity);
            if !self.batch.supports_list_batches() {
                return Err(Error::from(ProviderError::unsupported("batch listing")));
            }
            let batch = self.batch.clone();
            let base = self.base.clone();
            let limit = self.limit;
            let cursor = self.cursor;
            base.run(|base, token| {
                async move {
                    let headers = base.request_headers();
                    retry(&base.retry_policy, &token, |_| {
                        let options = BatchListOptions {
                            limit,
                            cursor: cursor.clone(),
                            provider_options: base.provider_options.clone(),
                            headers: headers.clone(),
                            cancellation: token.child_token(),
                        };
                        let batch = &batch;
                        async move { batch.do_list_batches(options).await.map_err(Error::from) }
                    })
                    .await
                }
                .instrument(span)
            })
            .await
        })
    }
}
