//! Azure OpenAI models with deployment routing and lazy credentials.
//!
//! # Examples
//!
//! ```no_run
//! use ferrin_azure::{AzureSettings, create_azure};
//! # fn main() -> Result<(), ferrin_spec::error::ProviderError> {
//! let provider = create_azure(AzureSettings::new("my-resource"))?;
//! let model = provider.responses("my-deployment");
//! # Ok(())
//! # }
//! ```
//!
//! # Attribution
//!
//! Azure routing and credential behavior is derived from the Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.), reimplemented in Rust. See `NOTICE`.

mod provider;
mod settings;
mod transcription;
mod transport;

pub use ferrin_openai::tools;
pub use provider::AzureProvider;
pub use provider::create_azure;
pub use settings::AzureSettings;
pub use settings::AzureUrlMode;
pub use settings::TokenProvider;
pub use settings::token_provider;
pub use transcription::AzureTranscriptionModel;
