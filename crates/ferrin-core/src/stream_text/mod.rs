//! Streaming text generation (`stream_text`).
//!
//! The step loop runs in a background task and emits [`StreamEvent`]s
//! through a bounded channel; the consumer side applies the user transforms,
//! accumulates step results and resolves the [`Completion`].
//!
//! Design: `docs/01-architecture/07-generation-loop-and-streaming.md` §3.

mod builder;
mod events;
mod pipeline;
mod result;
pub mod transforms;

pub use builder::ErrorDecision;
pub use builder::OnErrorFn;
pub(crate) use builder::StreamConfig;
pub use builder::StreamText;
pub use builder::stream_text;
pub use events::StreamErrorInfo;
pub use events::StreamEvent;
pub use result::Completion;
pub use result::EventStream;
pub use result::StreamTextResult;
pub use transforms::StreamTransform;
pub use transforms::TransformContext;
pub use transforms::smooth_stream;
