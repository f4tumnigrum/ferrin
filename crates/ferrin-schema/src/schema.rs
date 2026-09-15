//! The [`Schema`] abstraction: a JSON Schema plus a typed validator.

use std::fmt;
use std::sync::Arc;
use std::sync::LazyLock;

use ferrin_spec::error::TypeValidationError;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_json::json;

use crate::dialect::SchemaDialect;
use crate::transform::SchemaTransform;

type LazyValue = LazyLock<Value, Box<dyn FnOnce() -> Value + Send>>;
type Validate<T> = dyn Fn(Value) -> Result<T, TypeValidationError> + Send + Sync;

/// A JSON Schema paired with a function that validates a JSON value and
/// converts it into `T`.
///
/// The JSON Schema is produced lazily on first access and cached; cloning a
/// schema shares the cache and the validator.
pub struct Schema<T> {
    json_schema: Arc<LazyValue>,
    validate: Arc<Validate<T>>,
}

impl<T> Clone for Schema<T> {
    fn clone(&self) -> Self {
        Self {
            json_schema: Arc::clone(&self.json_schema),
            validate: Arc::clone(&self.validate),
        }
    }
}

impl<T> fmt::Debug for Schema<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Schema")
            .field("json_schema", &LazyLock::get(&self.json_schema))
            .finish_non_exhaustive()
    }
}

impl<T: DeserializeOwned + JsonSchema + 'static> Schema<T> {
    /// Derives the schema from `T` (draft-07, `additionalProperties: false`
    /// on objects) and validates by deserializing into `T`.
    #[must_use]
    pub fn derived() -> Self {
        Self::derived_with(SchemaDialect::default())
    }

    /// Like [`Schema::derived`] with an explicit dialect.
    #[must_use]
    pub fn derived_with(dialect: SchemaDialect) -> Self {
        Self::lazy(
            move || {
                let mut schema = dialect.generate::<T>();
                crate::transform::add_additional_properties_false(&mut schema);
                schema
            },
            deserialize_into::<T>,
        )
    }
}

impl<T: DeserializeOwned + 'static> Schema<T> {
    /// Uses a raw JSON Schema and validates by deserializing into `T`.
    ///
    /// With the `json-schema-validation` feature the value is first checked
    /// against the JSON Schema, so constraints that `serde` cannot express
    /// (ranges, patterns) are enforced as well.
    #[must_use]
    pub fn typed_from_json_schema(schema: Value) -> Self {
        let dynamic = Schema::<Value>::from_json_schema(schema);
        let dynamic_validate = Arc::clone(&dynamic.validate);
        Self {
            json_schema: dynamic.json_schema,
            validate: Arc::new(move |value| {
                let value = dynamic_validate(value)?;
                deserialize_into::<T>(value)
            }),
        }
    }
}

impl Schema<Value> {
    /// Uses a raw JSON Schema; the value is returned unchanged after
    /// validation.
    ///
    /// With the `json-schema-validation` feature the value is checked against
    /// the schema (compiled lazily on first validation); without it every
    /// value passes.
    #[must_use]
    pub fn from_json_schema(schema: Value) -> Self {
        let json_schema = Arc::new(LazyValue::new(Box::new(move || schema)));
        let validate = dynamic_validator(Arc::clone(&json_schema));
        Self {
            json_schema,
            validate,
        }
    }

    /// The empty object schema (`{type: object, properties: {},
    /// additionalProperties: false}`) used when no schema is given.
    #[must_use]
    pub fn empty_object() -> Self {
        Self::from_json_schema(json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false,
        }))
    }

    /// A schema that accepts any JSON value.
    #[must_use]
    pub fn any() -> Self {
        Self::from_json_schema(json!({}))
    }
}

impl<T: 'static> Schema<T> {
    /// Creates a schema from a lazily generated JSON Schema and a validator.
    pub fn lazy(
        json_schema: impl FnOnce() -> Value + Send + 'static,
        validate: impl Fn(Value) -> Result<T, TypeValidationError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            json_schema: Arc::new(LazyValue::new(Box::new(json_schema))),
            validate: Arc::new(validate),
        }
    }

    /// Creates a schema from a JSON Schema value and a validator.
    pub fn with_json_schema_and_validator(
        json_schema: Value,
        validate: impl Fn(Value) -> Result<T, TypeValidationError> + Send + Sync + 'static,
    ) -> Self {
        Self::lazy(move || json_schema, validate)
    }

    /// Replaces the validator, keeping the JSON Schema.
    #[must_use]
    pub fn with_validator(
        self,
        validate: impl Fn(Value) -> Result<T, TypeValidationError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            json_schema: self.json_schema,
            validate: Arc::new(validate),
        }
    }

    /// Returns a copy whose JSON Schema is rewritten by `transform`
    /// immediately; validation is unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SchemaError::UnsupportedTransform`] if the schema
    /// cannot be represented by the requested transform.
    pub fn transformed(&self, transform: SchemaTransform) -> Result<Self, crate::SchemaError> {
        let transformed = transform.applied(self.json_schema().clone())?;
        Ok(Self {
            json_schema: Arc::new(LazyValue::new(Box::new(move || transformed))),
            validate: Arc::clone(&self.validate),
        })
    }

    /// Returns a `Schema<Value>` that runs this schema's validation but
    /// yields the original JSON value.
    #[must_use]
    pub fn erased(&self) -> Schema<Value> {
        let validate = Arc::clone(&self.validate);
        Schema {
            json_schema: Arc::clone(&self.json_schema),
            validate: Arc::new(move |value| validate(value.clone()).map(|_| value)),
        }
    }
}

impl<T> Schema<T> {
    /// Returns the JSON Schema, generating it on first access.
    #[must_use]
    pub fn json_schema(&self) -> &Value {
        &self.json_schema
    }

    /// Validates `value` and converts it into `T`.
    ///
    /// # Errors
    ///
    /// Returns [`TypeValidationError`] when the value does not match.
    pub fn validate(&self, value: Value) -> Result<T, TypeValidationError> {
        (self.validate)(value)
    }
}

fn deserialize_into<T: DeserializeOwned>(value: Value) -> Result<T, TypeValidationError> {
    match serde_json::from_value::<T>(value.clone()) {
        Ok(typed) => Ok(typed),
        Err(error) => Err(TypeValidationError::new(value, error)),
    }
}

#[cfg(feature = "json-schema-validation")]
fn dynamic_validator(json_schema: Arc<LazyValue>) -> Arc<Validate<Value>> {
    use std::sync::OnceLock;

    use crate::validation::ValidationIssues;
    use crate::validation::Validator;

    let compiled: OnceLock<Result<Validator, String>> = OnceLock::new();
    Arc::new(move |value| {
        let validator = compiled
            .get_or_init(|| Validator::compile(&json_schema).map_err(|error| error.to_string()));
        match validator {
            Ok(validator) => validator
                .validate(&value)
                .map(|()| value.clone())
                .map_err(|issues| TypeValidationError::new(value, issues)),
            Err(message) => Err(TypeValidationError::new(
                value,
                ValidationIssues::message(message.clone()),
            )),
        }
    })
}

#[cfg(not(feature = "json-schema-validation"))]
fn dynamic_validator(_json_schema: Arc<LazyValue>) -> Arc<Validate<Value>> {
    Arc::new(Ok)
}
