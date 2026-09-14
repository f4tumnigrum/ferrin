//! JSON Schema dialects used for generation.

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::Value;

/// JSON Schema draft used when generating schemas from Rust types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SchemaDialect {
    /// Draft-07: the dialect providers and adapter transforms are built on.
    #[default]
    Draft07,
    /// Draft 2020-12.
    Draft2020_12,
}

impl SchemaDialect {
    /// Returns the `schemars` settings for this dialect.
    #[must_use]
    pub fn settings(self) -> SchemaSettings {
        match self {
            Self::Draft07 => SchemaSettings::draft07(),
            Self::Draft2020_12 => SchemaSettings::draft2020_12(),
        }
    }

    /// Generates the root schema of `T` as a JSON value.
    #[must_use]
    pub fn generate<T: JsonSchema>(self) -> Value {
        self.settings()
            .into_generator()
            .into_root_schema_for::<T>()
            .to_value()
    }

    /// Returns the `$schema` URI of this dialect.
    #[must_use]
    pub fn meta_schema(self) -> &'static str {
        match self {
            Self::Draft07 => "http://json-schema.org/draft-07/schema#",
            Self::Draft2020_12 => "https://json-schema.org/draft/2020-12/schema",
        }
    }
}
