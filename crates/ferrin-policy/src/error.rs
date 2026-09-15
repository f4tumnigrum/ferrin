//! Errors of policy evaluation.

use http::StatusCode;

/// Why a policy could not be evaluated.
///
/// Approval policies built with [`policy_approval`](crate::policy_approval)
/// turn these into a denial (or fall through, see
/// [`FailureMode`](crate::FailureMode)); the error is exposed for callers
/// that evaluate a [`PolicyClient`](crate::PolicyClient) directly.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PolicyError {
    /// The policy path is empty or contains an empty or blank segment.
    #[error("invalid policy path `{path}`: {message}")]
    InvalidPath {
        /// The rejected path.
        path: String,
        /// Explanation.
        message: String,
    },
    /// The policy server URL was rejected by the URL policy or cannot carry
    /// the data path.
    #[error("invalid policy url: {message}")]
    InvalidUrl {
        /// Explanation.
        message: String,
    },
    /// The policy input could not be encoded.
    #[error("invalid policy input: {message}")]
    InvalidInput {
        /// Explanation.
        message: String,
    },
    /// The request to the policy server failed below the HTTP status layer
    /// (connection, TLS, timeout, body limits).
    #[error("policy request to {host} failed: {message}")]
    Transport {
        /// Host of the policy server.
        host: String,
        /// Explanation.
        message: String,
    },
    /// The policy server answered with a non-success status.
    #[error("policy server returned HTTP {status}")]
    Status {
        /// The status code.
        status: StatusCode,
        /// The start of the response body (at most 1 KiB).
        body: String,
    },
    /// The response body is not the expected JSON document.
    #[error("policy response is not valid: {message}")]
    InvalidResponse {
        /// Explanation.
        message: String,
    },
    /// The embedded policy engine rejected the policy, the data document or
    /// the rule path, or failed during evaluation.
    #[error("policy engine error: {message}")]
    Engine {
        /// Engine message.
        message: String,
    },
}
