//! Provider metadata extraction hooks and helpers.

use std::fmt;
use std::sync::Arc;

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderMetadata;

/// Extracts provider-specific metadata from responses; implemented by
/// provider crates built on this one to surface non-standard fields.
pub trait MetadataExtractor: Send + Sync + fmt::Debug {
    /// Extracts metadata from a complete (non-streaming) response body.
    fn extract_metadata(&self, body: &JsonValue) -> Option<ProviderMetadata>;

    /// Creates the extractor that accumulates metadata over a stream.
    fn stream_extractor(&self) -> Box<dyn StreamMetadataExtractor>;
}

/// Accumulates metadata while a stream is consumed.
pub trait StreamMetadataExtractor: Send + fmt::Debug {
    /// Observes one parsed chunk.
    fn process_chunk(&mut self, chunk: &JsonValue);

    /// Builds the metadata once the stream ended.
    fn build_metadata(&mut self) -> Option<ProviderMetadata>;
}

/// Shared handle to a [`MetadataExtractor`].
pub type SharedMetadataExtractor = Arc<dyn MetadataExtractor>;

/// Provider metadata holding `value` under `key`.
#[must_use]
pub fn metadata_under(key: &str, value: JsonObject) -> ProviderMetadata {
    let mut map = ProviderMetadata::new();
    map.insert(key.to_owned(), value);
    map
}

/// Merges `extra` into `base`: objects under the same key are merged, later
/// entries win.
pub fn merge_metadata(base: &mut ProviderMetadata, extra: ProviderMetadata) {
    for (key, value) in extra {
        base.entry(key).or_default().extend(value);
    }
}

/// Seconds since the epoch (integer or float) to a timestamp.
#[must_use]
pub fn timestamp_from_seconds(seconds: Option<f64>) -> Option<chrono::DateTime<chrono::Utc>> {
    let seconds = seconds?;
    if !seconds.is_finite() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "epoch seconds are far below the i64 range"
    )]
    let whole = seconds.trunc() as i64;
    chrono::DateTime::from_timestamp(whole, 0)
}

/// Removes `null` entries from a JSON object.
#[must_use]
pub fn compact(mut object: JsonObject) -> JsonObject {
    object.retain(|_, value| !matches!(value, JsonValue::Null));
    object
}
