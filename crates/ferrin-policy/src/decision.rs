//! Decision documents and their normalization.
//!
//! Derived from the Vercel AI SDK decision normalization (Apache-2.0,
//! Copyright 2023 Vercel, Inc.), reimplemented in Rust with the bare boolean
//! form added.

use ferrin_core::generate_text::ApprovalStatus;
use ferrin_spec::JsonValue;
use serde::Deserialize;
use serde::Serialize;

/// Reason attached to denials of documents that are not recognized.
pub const UNRECOGNIZED_DECISION: &str = "unrecognized policy decision";

/// A normalized policy decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum PolicyDecision {
    /// The call may proceed without user approval.
    Allow {
        /// Reason recorded with the decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The call must not execute.
    Deny {
        /// Reason recorded with the decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// An external approver must decide.
    RequiresApproval {
        /// Reason shown to the approver.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The policy has no opinion; the tool's own `needs_approval` applies.
    NotApplicable,
}

impl PolicyDecision {
    /// Allow without reason.
    #[must_use]
    pub fn allow() -> Self {
        Self::Allow { reason: None }
    }

    /// Deny without reason.
    #[must_use]
    pub fn deny() -> Self {
        Self::Deny { reason: None }
    }

    /// Requires approval without reason.
    #[must_use]
    pub fn requires_approval() -> Self {
        Self::RequiresApproval { reason: None }
    }

    /// Attaches a reason (ignored for `NotApplicable`).
    #[must_use]
    pub fn with_reason(self, reason: impl Into<String>) -> Self {
        let reason = Some(reason.into());
        match self {
            Self::NotApplicable => Self::NotApplicable,
            Self::Allow { .. } => Self::Allow { reason },
            Self::Deny { .. } => Self::Deny { reason },
            Self::RequiresApproval { .. } => Self::RequiresApproval { reason },
        }
    }

    /// The reason, if any.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::NotApplicable => None,
            Self::Allow { reason } | Self::Deny { reason } | Self::RequiresApproval { reason } => {
                reason.as_deref()
            }
        }
    }

    /// Normalizes a raw decision document.
    ///
    /// Recognized forms:
    ///
    /// - `null`: not applicable (an undefined rule or a missing result).
    /// - `true` / `false`: allow / deny.
    /// - `{ "decision": "allow" | "deny" | "requires-approval" | "not-applicable", "reason"?: string }`.
    /// - `{ "allow": bool, "reason"?: string }` (legacy form).
    ///
    /// Anything else, including an unknown `decision` string, is a denial
    /// with the reason [`UNRECOGNIZED_DECISION`], so that a broken policy
    /// fails closed.
    #[must_use]
    pub fn normalize(raw: &JsonValue) -> Self {
        let unrecognized = || Self::Deny {
            reason: Some(UNRECOGNIZED_DECISION.to_owned()),
        };
        match raw {
            JsonValue::Null => Self::NotApplicable,
            JsonValue::Bool(true) => Self::allow(),
            JsonValue::Bool(false) => Self::deny(),
            JsonValue::Object(object) => {
                let reason = object
                    .get("reason")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned);
                if let Some(decision) = object.get("decision") {
                    return match decision.as_str() {
                        Some("allow") => Self::Allow { reason },
                        Some("deny") => Self::Deny { reason },
                        Some("requires-approval") => Self::RequiresApproval { reason },
                        Some("not-applicable") => Self::NotApplicable,
                        _ => unrecognized(),
                    };
                }
                match object.get("allow").and_then(JsonValue::as_bool) {
                    Some(true) => Self::Allow { reason },
                    Some(false) => Self::Deny { reason },
                    None => unrecognized(),
                }
            }
            _ => unrecognized(),
        }
    }

    /// Converts the decision into an approval status.
    ///
    /// `NotApplicable` becomes `None`, which makes an
    /// [`ApprovalPolicy`](ferrin_core::generate_text::ApprovalPolicy) fall
    /// through to the tool's own `needs_approval`.
    #[must_use]
    pub fn into_approval(self) -> Option<ApprovalStatus> {
        match self {
            Self::Allow { reason } => Some(ApprovalStatus::Approved { reason }),
            Self::Deny { reason } => Some(ApprovalStatus::Denied { reason }),
            Self::RequiresApproval { reason } => Some(ApprovalStatus::UserApproval { reason }),
            Self::NotApplicable => None,
        }
    }
}
