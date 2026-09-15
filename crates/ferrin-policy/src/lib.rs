//! Policy-based tool approval for Ferrin.
//!
//! A [`PolicyClient`] evaluates a policy path with a JSON input and returns
//! the raw decision document. [`policy_approval`] turns a client into an
//! [`ApprovalPolicy`](ferrin_core::generate_text::ApprovalPolicy): the tool
//! call, its input, the messages and the tools context become the policy
//! input, and the normalized [`PolicyDecision`] becomes the approval status.
//! [`capability_middleware`] filters the tools offered to the model through
//! the same client. [`shadow`] observes decisions without enforcing them and
//! [`with_default`] gives every call a decision.
//!
//! Clients: [`HttpPolicyClient`] speaks the OPA REST Data API
//! (`POST /v1/data/<path>`); [`RegoPolicyClient`] (feature `rego`) evaluates
//! Rego policies in-process with `regorus`. [`policy_client`] adapts a
//! closure, for tests and static rules.
//!
//! Design: `docs/01-architecture/18-policy-approval.md`, ADR 0020.
//!
//! # Attribution
//!
//! The decision document format, its normalization rules and the shadow and
//! capability patterns are derived from the Vercel AI SDK (Apache-2.0,
//! Copyright 2023 Vercel, Inc.) and reimplemented in Rust. See the `NOTICE`
//! file in the crate root.
//!
//! # Examples
//!
//! ```
//! use ferrin_core::generate_text::ApprovalStatus;
//! use ferrin_policy::PolicyDecision;
//! use ferrin_policy::policy_approval;
//! use ferrin_policy::policy_client;
//! use serde_json::json;
//!
//! // A decision document as returned by a policy server or a Rego rule.
//! let decision = PolicyDecision::normalize(&json!({
//!     "decision": "requires-approval",
//!     "reason": "writes outside the workspace"
//! }));
//! assert_eq!(
//!     decision.into_approval(),
//!     Some(ApprovalStatus::user_approval().with_reason("writes outside the workspace"))
//! );
//!
//! // A static in-process client; pass the policy to
//! // `generate_text(..).tool_approval(policy)`.
//! let client = policy_client(|_path, input| {
//!     Ok(json!({ "decision": if input["tool"]["name"] == "delete_file" { "deny" } else { "allow" } }))
//! });
//! let _policy = policy_approval(client, "ferrin/tools/decision");
//! ```

mod approval;
mod capability;
mod client;
mod decision;
mod diagnostics;
mod error;
mod http;
mod path;
#[cfg(feature = "rego")]
mod rego;
mod shadow;

pub use approval::FailureMode;
pub use approval::PolicyApproval;
pub use approval::ToInputFn;
pub use approval::WithDefault;
pub use approval::default_input;
pub use approval::policy_approval;
pub use approval::with_default;
pub use capability::CapabilityInputFn;
pub use capability::CapabilityMiddleware;
pub use capability::capability_middleware;
pub use capability::default_capability_input;
pub use capability::parse_allowlist;
pub use client::PolicyClient;
pub use client::PolicyClientFn;
pub use client::SharedPolicyClient;
pub use client::policy_client;
pub use decision::PolicyDecision;
pub use decision::UNRECOGNIZED_DECISION;
pub use error::PolicyError;
pub use http::DEFAULT_MAX_RESPONSE_BYTES;
pub use http::HttpPolicyClient;
pub use http::HttpPolicyClientBuilder;
#[cfg(feature = "rego")]
pub use rego::RegoPolicyClient;
#[cfg(feature = "rego")]
pub use rego::RegoPolicyClientBuilder;
pub use shadow::Enforcement;
pub use shadow::OnDecisionFn;
pub use shadow::Shadow;
pub use shadow::shadow;
