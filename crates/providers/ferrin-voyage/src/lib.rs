//! Voyage reranking provider for Ferrin.
//!
//! # Examples
//!
//! ```no_run
//! use ferrin_spec::RerankingModel;
//! use ferrin_spec::reranking_model::{RerankDocuments, RerankOptions};
//! use ferrin_voyage::{VoyageSettings, create_voyage};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let provider = create_voyage(VoyageSettings::default())?;
//! let result = provider.reranking("rerank-2.5").do_rerank(RerankOptions::new(
//!     "Rust async runtimes",
//!     RerankDocuments::Text { values: vec!["Tokio provides an async runtime.".into()] },
//! )).await?;
//! # Ok(())
//! # }
//! ```
//!
//! Settings and verification limits: `docs/providers/voyage.md`.
//!
//! # Attribution
//!
//! Portions of this crate are derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), translated from TypeScript to Rust and
//! modified. See the crate's `NOTICE` file.

mod config;
mod error;
mod options;
mod provider;
mod reranking;

pub use config::SharedConfig;
pub use config::VoyageConfig;
pub use options::VoyageRerankingOptions;
pub use provider::VoyageProvider;
pub use provider::VoyageSettings;
pub use provider::create_voyage;
pub use reranking::VoyageRerankingModel;
