//! Response metadata helpers.

use chrono::DateTime;
use ferrin_spec::ModelId;
use ferrin_spec::ResponseMetadata;

/// Builds [`ResponseMetadata`] from the usual provider fields: response id,
/// model name and creation time in Unix seconds.
#[must_use]
pub fn response_metadata(
    id: Option<String>,
    model: Option<String>,
    created_unix_seconds: Option<i64>,
) -> ResponseMetadata {
    ResponseMetadata {
        id,
        timestamp: created_unix_seconds.and_then(|seconds| DateTime::from_timestamp(seconds, 0)),
        model_id: model.map(ModelId::from),
        headers: None,
        body: None,
    }
}
