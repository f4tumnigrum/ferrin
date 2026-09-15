//! The policy client abstraction.

use std::fmt;
use std::sync::Arc;

use ferrin_spec::BoxFuture;
use ferrin_spec::JsonValue;

use crate::error::PolicyError;

/// Evaluates policies: a path and a JSON input in, a raw decision document
/// out.
///
/// The raw document is normalized by
/// [`PolicyDecision::normalize`](crate::PolicyDecision::normalize) (approval)
/// or [`parse_allowlist`](crate::parse_allowlist) (capabilities). Clients
/// return `JsonValue::Null` when the policy produced no value (an undefined
/// rule), never an error.
pub trait PolicyClient: Send + Sync + 'static {
    /// Evaluates the policy at `path` with `input`.
    fn evaluate<'a>(
        &'a self,
        path: &'a str,
        input: JsonValue,
    ) -> BoxFuture<'a, Result<JsonValue, PolicyError>>;
}

impl<T: PolicyClient + ?Sized> PolicyClient for Arc<T> {
    fn evaluate<'a>(
        &'a self,
        path: &'a str,
        input: JsonValue,
    ) -> BoxFuture<'a, Result<JsonValue, PolicyError>> {
        (**self).evaluate(path, input)
    }
}

/// Shared reference to a policy client.
pub type SharedPolicyClient = Arc<dyn PolicyClient>;

/// Adapter turning a synchronous closure into a [`PolicyClient`].
pub struct PolicyClientFn<F>(F);

/// Wraps a synchronous closure `(path, input) -> decision` as a policy
/// client, for tests and static rules.
pub fn policy_client<F>(f: F) -> PolicyClientFn<F>
where
    F: Fn(&str, JsonValue) -> Result<JsonValue, PolicyError> + Send + Sync + 'static,
{
    PolicyClientFn(f)
}

impl<F> fmt::Debug for PolicyClientFn<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PolicyClientFn(..)")
    }
}

impl<F> PolicyClient for PolicyClientFn<F>
where
    F: Fn(&str, JsonValue) -> Result<JsonValue, PolicyError> + Send + Sync + 'static,
{
    fn evaluate<'a>(
        &'a self,
        path: &'a str,
        input: JsonValue,
    ) -> BoxFuture<'a, Result<JsonValue, PolicyError>> {
        let result = (self.0)(path, input);
        Box::pin(async move { result })
    }
}
