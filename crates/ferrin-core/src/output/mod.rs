//! Structured output strategies for `generate_text` and `stream_text`.
//!
//! An [`Output`] decides the response format sent to the model and parses
//! the final (and, when streaming, partial) text.

use std::fmt;
use std::sync::Arc;

use ferrin_schema::JsonSchema;
use ferrin_schema::Schema;
use ferrin_spec::FinishReason;
use ferrin_spec::JsonValue;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Usage;
use serde::de::DeserializeOwned;

use crate::error::Error;

mod strategies;

pub use strategies::ArrayOutput;
pub use strategies::ChoiceOutput;
pub use strategies::JsonOutput;
pub use strategies::ObjectOutput;
pub use strategies::TextOutput;

/// Response context handed to [`OutputHandler::parse_complete`] (used to
/// populate `NoObjectGenerated` errors).
#[derive(Debug, Clone, PartialEq)]
pub struct OutputContext {
    /// Response metadata of the final step.
    pub response: ResponseMetadata,
    /// Usage of the final step.
    pub usage: Usage,
    /// Finish reason of the final step.
    pub finish_reason: FinishReason,
}

/// Implements an output strategy for output type `O`.
///
/// Implement this to add custom strategies; the built-in ones are exposed
/// through [`Output`].
pub trait OutputHandler<O>: Send + Sync + 'static {
    /// The response format sent to the model.
    fn response_format(&self) -> Option<ResponseFormat>;

    /// Whether the call must produce an output (`false` only for [`NoOutput`]).
    fn wants_output(&self) -> bool {
        true
    }

    /// Parses the complete text of the final step.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoObjectGenerated`] when the text does not satisfy the
    /// strategy.
    fn parse_complete(&self, text: &str, ctx: &OutputContext) -> Result<O, Error>;

    /// Parses partial text during streaming; `None` when nothing usable is
    /// available yet.
    fn parse_partial(&self, text: &str) -> Option<JsonValue>;

    /// Converts a partial JSON value into the typed output when it already
    /// satisfies the schema.
    fn typed_partial(&self, _value: &JsonValue) -> Option<O> {
        None
    }

    /// Array strategies: the complete, validated elements contained in the
    /// partial text so far.
    fn parse_elements(&self, _text: &str) -> Option<Vec<JsonValue>> {
        None
    }
}

/// The strategy used when no output is requested: no response format, no
/// parsing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOutput;

impl OutputHandler<()> for NoOutput {
    fn response_format(&self) -> Option<ResponseFormat> {
        None
    }

    fn wants_output(&self) -> bool {
        false
    }

    fn parse_complete(&self, _text: &str, _ctx: &OutputContext) -> Result<(), Error> {
        Ok(())
    }

    fn parse_partial(&self, _text: &str) -> Option<JsonValue> {
        None
    }
}

/// A structured output specification producing values of type `T`.
pub struct Output<T> {
    handler: Arc<dyn OutputHandler<T>>,
}

impl<T> Clone for Output<T> {
    fn clone(&self) -> Self {
        Self {
            handler: Arc::clone(&self.handler),
        }
    }
}

impl<T> fmt::Debug for Output<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Output(..)")
    }
}

impl<T> Output<T> {
    /// Wraps a custom strategy.
    pub fn custom(handler: impl OutputHandler<T>) -> Self {
        Self {
            handler: Arc::new(handler),
        }
    }

    /// The underlying handler.
    #[must_use]
    pub fn handler(&self) -> Arc<dyn OutputHandler<T>> {
        Arc::clone(&self.handler)
    }
}

impl Output<String> {
    /// Plain text (the default when no output is configured).
    #[must_use]
    pub fn text() -> Self {
        Self::custom(TextOutput)
    }

    /// One of `options`, enforced through a JSON schema enum.
    #[must_use]
    pub fn choice(options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::custom(ChoiceOutput::new(options))
    }
}

impl<T: DeserializeOwned + JsonSchema + Send + Sync + 'static> Output<T> {
    /// A JSON object described by `T`'s derived schema.
    #[must_use]
    pub fn object() -> Self {
        Self::custom(ObjectOutput::new(Schema::<T>::derived()))
    }
}

impl<T: DeserializeOwned + Send + Sync + 'static> Output<T> {
    /// A JSON object validated by `schema`.
    #[must_use]
    pub fn object_with(schema: Schema<T>) -> Self {
        Self::custom(ObjectOutput::new(schema))
    }
}

impl<T: DeserializeOwned + JsonSchema + Send + Sync + 'static> Output<Vec<T>> {
    /// An array of elements described by `T`'s derived schema.
    #[must_use]
    pub fn array() -> Self {
        Self::custom(ArrayOutput::new(Schema::<T>::derived()))
    }
}

impl<T: DeserializeOwned + Send + Sync + 'static> Output<Vec<T>> {
    /// An array of elements validated by `element`.
    #[must_use]
    pub fn array_with(element: Schema<T>) -> Self {
        Self::custom(ArrayOutput::new(element))
    }
}

impl Output<JsonValue> {
    /// Unconstrained JSON.
    #[must_use]
    pub fn json() -> Self {
        Self::custom(JsonOutput::new(None))
    }

    /// JSON validated by a raw JSON schema.
    #[must_use]
    pub fn json_with_schema(schema: JsonValue) -> Self {
        Self::custom(JsonOutput::new(Some(schema)))
    }
}

/// Marker for outputs that stream element by element (`Vec<T>`).
pub trait ArrayElements {
    /// The element type.
    type Element;
}

impl<T> ArrayElements for Vec<T> {
    type Element = T;
}

/// A partial structured output published while streaming.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialOutput<T> {
    /// The repaired partial JSON value.
    pub value: JsonValue,
    /// The typed value, when the partial JSON already satisfies the schema.
    pub typed: Option<T>,
}
