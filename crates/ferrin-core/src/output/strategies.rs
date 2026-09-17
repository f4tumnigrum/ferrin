//! Built-in output strategies.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use std::fmt;

use ferrin_schema::Schema;
use ferrin_schema::partial_json::PartialParseState;
use ferrin_schema::partial_json::parse_partial;
use ferrin_spec::JsonValue;
use ferrin_spec::ResponseFormat;
use serde::de::DeserializeOwned;
use serde_json::json;

use super::OutputContext;
use super::OutputHandler;
use crate::error::Error;
use crate::error::NoObjectGeneratedDetails;

fn no_object(
    message: &str,
    text: &str,
    ctx: &OutputContext,
    cause: Option<crate::error::BoxError>,
) -> Error {
    Error::no_object_generated(NoObjectGeneratedDetails {
        message: message.to_owned(),
        text: Some(text.to_owned()),
        response: ctx.response.clone(),
        usage: ctx.usage.clone(),
        finish_reason: ctx.finish_reason.clone(),
        cause,
    })
}

fn parse_json(text: &str, ctx: &OutputContext) -> Result<JsonValue, Error> {
    ferrin_schema::json::parse(text).map_err(|error| {
        no_object(
            "could not parse the response",
            text,
            ctx,
            Some(Box::new(error)),
        )
    })
}

fn partial_value(text: &str) -> Option<(JsonValue, PartialParseState)> {
    let parsed = parse_partial(text);
    match parsed.state {
        PartialParseState::FailedParse => None,
        state => parsed.value.map(|value| (value, state)),
    }
}

/// Plain text.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextOutput;

impl OutputHandler<String> for TextOutput {
    fn response_format(&self) -> Option<ResponseFormat> {
        Some(ResponseFormat::Text)
    }

    fn parse_complete(&self, text: &str, _ctx: &OutputContext) -> Result<String, Error> {
        Ok(text.to_owned())
    }

    fn parse_partial(&self, text: &str) -> Option<JsonValue> {
        Some(JsonValue::String(text.to_owned()))
    }

    fn typed_partial(&self, value: &JsonValue) -> Option<String> {
        value.as_str().map(str::to_owned)
    }
}

/// A JSON object validated by a schema.
pub struct ObjectOutput<T> {
    schema: Schema<T>,
    name: Option<String>,
    description: Option<String>,
}

impl<T> ObjectOutput<T> {
    /// Creates the strategy.
    #[must_use]
    pub fn new(schema: Schema<T>) -> Self {
        Self {
            schema,
            name: None,
            description: None,
        }
    }

    /// Names the output for providers that accept a schema name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Describes the output for providers that accept a schema description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl<T> fmt::Debug for ObjectOutput<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectOutput")
            .field("schema", self.schema.json_schema())
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

impl<T: DeserializeOwned + Send + Sync + 'static> OutputHandler<T> for ObjectOutput<T> {
    fn response_format(&self) -> Option<ResponseFormat> {
        Some(ResponseFormat::Json {
            schema: Some(self.schema.json_schema().clone()),
            name: self.name.clone(),
            description: self.description.clone(),
        })
    }

    fn parse_complete(&self, text: &str, ctx: &OutputContext) -> Result<T, Error> {
        let value = parse_json(text, ctx)?;
        self.schema.validate(value).map_err(|error| {
            no_object(
                "response did not match schema",
                text,
                ctx,
                Some(Box::new(error)),
            )
        })
    }

    fn parse_partial(&self, text: &str) -> Option<JsonValue> {
        partial_value(text).map(|(value, _)| value)
    }

    fn typed_partial(&self, value: &JsonValue) -> Option<T> {
        self.schema.validate(value.clone()).ok()
    }
}

/// An array of schema-validated elements, wrapped in `{ "elements": [...] }`
/// on the wire.
pub struct ArrayOutput<T> {
    element: Schema<T>,
    min_items: Option<usize>,
    max_items: Option<usize>,
    name: Option<String>,
    description: Option<String>,
}

impl<T> ArrayOutput<T> {
    /// Creates the strategy.
    #[must_use]
    pub fn new(element: Schema<T>) -> Self {
        Self {
            element,
            min_items: None,
            max_items: None,
            name: None,
            description: None,
        }
    }

    /// Requires at least `n` elements.
    #[must_use]
    pub fn min_items(mut self, n: usize) -> Self {
        self.min_items = Some(n);
        self
    }

    /// Allows at most `n` elements.
    #[must_use]
    pub fn max_items(mut self, n: usize) -> Self {
        self.max_items = Some(n);
        self
    }

    /// Names the output.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Describes the output.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn wrapper_schema(&self) -> JsonValue {
        let mut element = self.element.json_schema().clone();
        super::local_refs::relocate(&mut element, "#/properties/elements/items");
        let mut elements = json!({
            "type": "array",
            "items": element,
        });
        if let Some(min) = self.min_items {
            elements["minItems"] = json!(min);
        }
        if let Some(max) = self.max_items {
            elements["maxItems"] = json!(max);
        }
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": { "elements": elements },
            "required": ["elements"],
            "additionalProperties": false,
        })
    }

    fn elements_of(value: &JsonValue) -> Option<&Vec<JsonValue>> {
        value.as_object()?.get("elements")?.as_array()
    }
}

impl<T> fmt::Debug for ArrayOutput<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArrayOutput")
            .field("element", self.element.json_schema())
            .field("min_items", &self.min_items)
            .field("max_items", &self.max_items)
            .finish_non_exhaustive()
    }
}

impl<T: DeserializeOwned + Send + Sync + 'static> OutputHandler<Vec<T>> for ArrayOutput<T> {
    fn validate_configuration(&self) -> Result<(), Error> {
        if let (Some(min), Some(max)) = (self.min_items, self.max_items)
            && min > max
        {
            return Err(Error::invalid_argument(
                "min_items",
                "min_items must not exceed max_items",
            ));
        }
        Ok(())
    }

    fn max_elements(&self) -> Option<usize> {
        self.max_items
    }

    fn response_format(&self) -> Option<ResponseFormat> {
        Some(ResponseFormat::Json {
            schema: Some(self.wrapper_schema()),
            name: self.name.clone(),
            description: self.description.clone(),
        })
    }

    fn parse_complete(&self, text: &str, ctx: &OutputContext) -> Result<Vec<T>, Error> {
        let value = parse_json(text, ctx)?;
        let Some(elements) = Self::elements_of(&value) else {
            return Err(no_object(
                "response must be an object with an elements array",
                text,
                ctx,
                None,
            ));
        };
        if let Some(min) = self.min_items
            && elements.len() < min
        {
            return Err(no_object(
                &format!("elements array must contain at least {min} items"),
                text,
                ctx,
                None,
            ));
        }
        if let Some(max) = self.max_items
            && elements.len() > max
        {
            return Err(no_object(
                &format!("elements array must contain at most {max} items"),
                text,
                ctx,
                None,
            ));
        }
        elements
            .iter()
            .map(|element| {
                self.element.validate(element.clone()).map_err(|error| {
                    no_object(
                        "response did not match schema",
                        text,
                        ctx,
                        Some(Box::new(error)),
                    )
                })
            })
            .collect()
    }

    fn parse_partial(&self, text: &str) -> Option<JsonValue> {
        self.parse_elements(text).map(JsonValue::Array)
    }

    fn typed_partial(&self, value: &JsonValue) -> Option<Vec<T>> {
        value
            .as_array()?
            .iter()
            .map(|element| self.element.validate(element.clone()).ok())
            .collect()
    }

    fn parse_elements(&self, text: &str) -> Option<Vec<JsonValue>> {
        let (value, state) = partial_value(text)?;
        let elements = Self::elements_of(&value)?;
        let complete = match state {
            PartialParseState::RepairedParse if !elements.is_empty() => {
                &elements[..elements.len() - 1]
            }
            _ => elements.as_slice(),
        };
        let mut validated = Vec::with_capacity(complete.len());
        for element in complete {
            if self.element.validate(element.clone()).is_ok() {
                validated.push(element.clone());
            }
        }
        Some(validated)
    }

    fn parse_typed_elements(&self, text: &str) -> Option<Vec<T>> {
        let (value, state) = partial_value(text)?;
        let elements = Self::elements_of(&value)?;
        let complete = match state {
            PartialParseState::RepairedParse if !elements.is_empty() => {
                &elements[..elements.len() - 1]
            }
            _ => elements.as_slice(),
        };
        Some(
            complete
                .iter()
                .filter_map(|element| self.element.validate(element.clone()).ok())
                .collect(),
        )
    }
}

/// One of a fixed set of string options.
#[derive(Debug, Clone)]
pub struct ChoiceOutput {
    options: Vec<String>,
    name: Option<String>,
    description: Option<String>,
}

impl ChoiceOutput {
    /// Creates the strategy.
    #[must_use]
    pub fn new(options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            name: None,
            description: None,
        }
    }

    /// Names the output.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Describes the output.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn result_of(value: &JsonValue) -> Option<&str> {
        value.as_object()?.get("result")?.as_str()
    }
}

impl OutputHandler<String> for ChoiceOutput {
    fn response_format(&self) -> Option<ResponseFormat> {
        Some(ResponseFormat::Json {
            schema: Some(json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object",
                "properties": { "result": { "type": "string", "enum": self.options } },
                "required": ["result"],
                "additionalProperties": false,
            })),
            name: self.name.clone(),
            description: self.description.clone(),
        })
    }

    fn parse_complete(&self, text: &str, ctx: &OutputContext) -> Result<String, Error> {
        let value = parse_json(text, ctx)?;
        let Some(result) = Self::result_of(&value) else {
            return Err(no_object(
                "response must be an object with a result string",
                text,
                ctx,
                None,
            ));
        };
        if !self.options.iter().any(|option| option == result) {
            return Err(no_object(
                "response did not match one of the options",
                text,
                ctx,
                None,
            ));
        }
        Ok(result.to_owned())
    }

    fn parse_partial(&self, text: &str) -> Option<JsonValue> {
        let (value, state) = partial_value(text)?;
        let partial = Self::result_of(&value)?;
        let matches: Vec<&String> = self
            .options
            .iter()
            .filter(|option| option.starts_with(partial))
            .collect();
        match state {
            PartialParseState::SuccessfulParse => matches
                .iter()
                .any(|option| option.as_str() == partial)
                .then(|| JsonValue::String(partial.to_owned())),
            _ => (matches.len() == 1).then(|| JsonValue::String(matches[0].clone())),
        }
    }

    fn typed_partial(&self, value: &JsonValue) -> Option<String> {
        value.as_str().map(str::to_owned)
    }
}

/// Unconstrained (or raw-schema-validated) JSON.
#[derive(Debug, Clone)]
pub struct JsonOutput {
    schema: Option<Schema<JsonValue>>,
    name: Option<String>,
    description: Option<String>,
}

impl JsonOutput {
    /// Creates the strategy; `schema` is a raw JSON schema.
    #[must_use]
    pub fn new(schema: Option<JsonValue>) -> Self {
        Self {
            schema: schema.map(Schema::from_json_schema),
            name: None,
            description: None,
        }
    }

    /// Names the output.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Describes the output.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl OutputHandler<JsonValue> for JsonOutput {
    fn response_format(&self) -> Option<ResponseFormat> {
        Some(ResponseFormat::Json {
            schema: self
                .schema
                .as_ref()
                .map(|schema| schema.json_schema().clone()),
            name: self.name.clone(),
            description: self.description.clone(),
        })
    }

    fn parse_complete(&self, text: &str, ctx: &OutputContext) -> Result<JsonValue, Error> {
        let value = parse_json(text, ctx)?;
        match &self.schema {
            Some(schema) => schema.validate(value).map_err(|error| {
                no_object(
                    "response did not match schema",
                    text,
                    ctx,
                    Some(Box::new(error)),
                )
            }),
            None => Ok(value),
        }
    }

    fn parse_partial(&self, text: &str) -> Option<JsonValue> {
        partial_value(text).map(|(value, _)| value)
    }

    fn typed_partial(&self, value: &JsonValue) -> Option<JsonValue> {
        match &self.schema {
            Some(schema) => schema.validate(value.clone()).ok(),
            None => Some(value.clone()),
        }
    }
}
