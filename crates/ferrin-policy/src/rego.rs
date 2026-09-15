//! In-process Rego evaluation with `regorus`.

use std::fmt;

use ferrin_spec::BoxFuture;
use ferrin_spec::JsonValue;
use regorus::Engine;
use regorus::Value;

use crate::client::PolicyClient;
use crate::error::PolicyError;
use crate::path::PolicyPath;

/// Evaluates Rego policies in-process.
///
/// Policies and data documents are loaded once through the builder; every
/// evaluation clones the prepared engine, sets the input and evaluates the
/// rule `data.<path>`. An undefined rule value yields `JsonValue::Null`
/// (not applicable); a rule path that does not exist is an
/// [`PolicyError::Engine`] error, which approval policies treat as a
/// denial.
pub struct RegoPolicyClient {
    engine: Engine,
    packages: Vec<String>,
}

impl fmt::Debug for RegoPolicyClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegoPolicyClient")
            .field("packages", &self.packages)
            .finish_non_exhaustive()
    }
}

impl RegoPolicyClient {
    /// Starts building a client.
    #[must_use]
    pub fn builder() -> RegoPolicyClientBuilder {
        RegoPolicyClientBuilder::default()
    }

    /// Creates a client from a single policy module.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::Engine`] when the policy does not parse.
    pub fn from_policy(
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<Self, PolicyError> {
        Self::builder().policy(name, source).build()
    }

    /// The `data.<package>` paths of the loaded policy modules.
    #[must_use]
    pub fn packages(&self) -> &[String] {
        &self.packages
    }

    #[tracing::instrument(skip_all, fields(path))]
    fn evaluate_sync(&self, path: &str, input: JsonValue) -> Result<JsonValue, PolicyError> {
        let path = PolicyPath::parse(path)?;
        let mut engine = self.engine.clone();
        engine.set_input(Value::from(input));
        let value = engine.eval_rule(path.rego_rule()).map_err(engine_error)?;
        to_json(value)
    }
}

impl PolicyClient for RegoPolicyClient {
    fn evaluate<'a>(
        &'a self,
        path: &'a str,
        input: JsonValue,
    ) -> BoxFuture<'a, Result<JsonValue, PolicyError>> {
        let result = self.evaluate_sync(path, input);
        Box::pin(async move { result })
    }
}

fn engine_error(error: impl fmt::Display) -> PolicyError {
    PolicyError::Engine {
        message: error.to_string(),
    }
}

fn to_json(value: Value) -> Result<JsonValue, PolicyError> {
    if matches!(value, Value::Undefined) {
        return Ok(JsonValue::Null);
    }
    serde_json::to_value(&value).map_err(|error| PolicyError::Engine {
        message: format!("could not convert the decision to JSON: {error}"),
    })
}

/// Builder of a [`RegoPolicyClient`].
#[derive(Debug, Default)]
pub struct RegoPolicyClientBuilder {
    policies: Vec<(String, String)>,
    data: Vec<JsonValue>,
}

impl RegoPolicyClientBuilder {
    /// Adds a policy module; `name` labels it in error messages.
    #[must_use]
    pub fn policy(mut self, name: impl Into<String>, source: impl Into<String>) -> Self {
        self.policies.push((name.into(), source.into()));
        self
    }

    /// Adds a data document (merged into `data` with earlier documents).
    #[must_use]
    pub fn data(mut self, data: JsonValue) -> Self {
        self.data.push(data);
        self
    }

    /// Parses the policies and loads the data.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::Engine`] when a policy does not parse or a data
    /// document cannot be merged.
    pub fn build(self) -> Result<RegoPolicyClient, PolicyError> {
        let mut engine = Engine::new();
        let mut packages = Vec::with_capacity(self.policies.len());
        for (name, source) in self.policies {
            packages.push(engine.add_policy(name, source).map_err(engine_error)?);
        }
        for data in self.data {
            engine.add_data(Value::from(data)).map_err(engine_error)?;
        }
        Ok(RegoPolicyClient { engine, packages })
    }
}
