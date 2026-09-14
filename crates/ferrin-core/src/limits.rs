//! Numeric limits shared by the core.

/// Capacity of the channel that injects tool results into a stream.
pub(crate) const TOOL_RESULT_CHANNEL_CAPACITY: usize = 64;

/// Capacity of the channel between the pipeline task and the consumer.
pub(crate) const EVENT_CHANNEL_CAPACITY: usize = 64;

/// Default number of concurrent URL downloads during prompt conversion.
pub(crate) const DEFAULT_MAX_PARALLEL_DOWNLOADS: usize = 8;
