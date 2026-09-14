//! Batch processing: [`start_batch`], [`get_batch_status`],
//! [`get_batch_results`], [`cancel_batch`] and [`list_batches`].
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §8.

mod operations;
mod request;
mod result;
mod start;

use ferrin_spec::BatchRef;

pub use ferrin_spec::BatchId;
pub use ferrin_spec::batch::BatchCancelResult;
pub use ferrin_spec::batch::BatchError;
pub use ferrin_spec::batch::BatchItem;
pub use ferrin_spec::batch::BatchListItem;
pub use ferrin_spec::batch::BatchListResult;
pub use ferrin_spec::batch::BatchRequest as ModelBatchRequest;
pub use ferrin_spec::batch::BatchRequestCounts;
pub use ferrin_spec::batch::BatchStartOptions;
pub use ferrin_spec::batch::BatchStartResult;
pub use ferrin_spec::batch::BatchState;
pub use ferrin_spec::batch::BatchStatus;
pub use ferrin_spec::batch::BatchWarning;
pub use operations::CancelBatch;
pub use operations::GetBatchResults;
pub use operations::GetBatchStatus;
pub use operations::ListBatches;
pub use operations::cancel_batch;
pub use operations::get_batch_results;
pub use operations::get_batch_status;
pub use operations::list_batches;
pub use request::BatchRequest;
pub use request::ImageBatchRequest;
pub use request::TextBatchRequest;
pub use result::BatchResultItem;
pub use result::BatchResults;
pub use result::ImageBatchResult;
pub use result::TextBatchResult;
pub use start::StartBatch;
pub use start::start_batch;

use crate::telemetry::ModelIdentity;

pub(super) fn service_identity(batch: &BatchRef) -> ModelIdentity {
    ModelIdentity::new(batch.provider().clone(), "batch")
}
