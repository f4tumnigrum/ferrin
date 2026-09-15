//! Built-in middleware.
//!
//! | Constructor | Behaviour |
//! | --- | --- |
//! | [`default_settings`] | fills unset call options from defaults |
//! | [`default_instructions`] | prepends system instructions when the prompt has none |
//! | [`extract_reasoning`] | turns `<tag>..</tag>` text into reasoning parts |
//! | [`simulate_streaming`] | serves `do_stream` from a `do_generate` call |
//! | [`extract_json`] | strips markdown code fences around JSON text |
//! | [`add_tool_input_examples`] | appends tool input examples to descriptions |
//! | [`default_embedding_settings`] | fills unset embedding call options from defaults |

mod add_tool_input_examples;
mod default_embedding_settings;
mod default_instructions;
mod default_settings;
mod extract_json;
mod extract_reasoning;
mod simulate_streaming;

pub use add_tool_input_examples::AddToolInputExamples;
pub use add_tool_input_examples::ExampleFormatFn;
pub use add_tool_input_examples::add_tool_input_examples;
pub use default_embedding_settings::DefaultEmbeddingSettings;
pub use default_embedding_settings::EmbeddingDefaults;
pub use default_embedding_settings::default_embedding_settings;
pub use default_instructions::DefaultInstructions;
pub use default_instructions::default_instructions;
pub use default_settings::CallDefaults;
pub use default_settings::DefaultSettings;
pub use default_settings::default_settings;
pub use default_settings::merge_json_objects;
pub use extract_json::ExtractJson;
pub use extract_json::JsonTransformFn;
pub use extract_json::extract_json;
pub use extract_json::strip_json_fences;
pub use extract_reasoning::ExtractReasoning;
pub use extract_reasoning::extract_reasoning;
pub use simulate_streaming::SimulateStreaming;
pub use simulate_streaming::simulate_parts;
pub use simulate_streaming::simulate_streaming;
