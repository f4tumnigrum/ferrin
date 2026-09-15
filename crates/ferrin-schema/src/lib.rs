//! Ferrin JSON Schema support.
//!
//! - [`Schema`]: a JSON Schema plus a typed validation function, created from
//!   a Rust type (`schemars` derive + `serde` deserialization), from a raw JSON
//!   Schema (validated with `jsonschema` when the `json-schema-validation`
//!   feature is enabled) or from a custom validator.
//! - [`SchemaDialect`]: draft-07 (default) or 2020-12 generation settings.
//! - [`SchemaTransform`]: provider-oriented schema rewrites
//!   (`additionalProperties: false`, OpenAI strict mode).
//! - [`partial_json`]: repair and parse truncated JSON produced by streaming
//!   models.
//! - [`json`]: JSON parsing with size and depth limits.
//!
//! Design: `docs/01-architecture/08-structured-output.md`, ADR 0004.
//!
//! # Attribution
//!
//! Portions of this crate are derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), translated from TypeScript to Rust and
//! modified. See the `NOTICE` file in the crate root.

pub mod dialect;
mod error;
pub mod json;
pub mod partial_json;
pub mod schema;
pub mod transform;
mod transform_refs;
pub mod validation;

pub use dialect::SchemaDialect;
pub use error::SchemaError;
pub use ferrin_spec::error::JsonParseError;
pub use ferrin_spec::error::TypeValidationContext;
pub use ferrin_spec::error::TypeValidationError;
pub use json::ParseLimits;
pub use partial_json::PartialParse;
pub use partial_json::PartialParseState;
pub use schema::Schema;
pub use schemars;
pub use schemars::JsonSchema;
pub use transform::SchemaTransform;
pub use validation::ValidationIssue;
pub use validation::ValidationIssues;
