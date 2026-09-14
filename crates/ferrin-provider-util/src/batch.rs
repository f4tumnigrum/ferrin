//! Batch helpers.

use ferrin_spec::batch::BatchRequestCounts;

/// Builds request counts when every count is present and they add up.
#[must_use]
pub fn normalize_batch_request_counts(
    total: Option<u64>,
    pending: Option<u64>,
    completed: Option<u64>,
    failed: Option<u64>,
) -> Option<BatchRequestCounts> {
    let (total, pending, completed, failed) = (total?, pending?, completed?, failed?);
    let sum = pending.checked_add(completed)?.checked_add(failed)?;
    (sum == total).then_some(BatchRequestCounts {
        total,
        pending,
        completed,
        failed,
    })
}
