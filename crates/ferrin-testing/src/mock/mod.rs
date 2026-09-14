//! Mock models with scripted responses and call recording.

mod language_model;

pub use language_model::MockCallKind;
pub use language_model::MockLanguageModel;
pub use language_model::MockLanguageModelBuilder;
pub use language_model::RecordedCall;
pub use language_model::api_call_error;
