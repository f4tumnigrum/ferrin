//! Payload-free labels for policy diagnostics.

use ferrin_core::generate_text::ApprovalStatus;

use crate::PolicyDecision;

pub(crate) fn decision_kind(decision: &PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Allow { .. } => "allow",
        PolicyDecision::Deny { .. } => "deny",
        PolicyDecision::RequiresApproval { .. } => "requires-approval",
        PolicyDecision::NotApplicable => "not-applicable",
    }
}

pub(crate) fn status_kind(status: Option<&ApprovalStatus>) -> &'static str {
    match status {
        Some(ApprovalStatus::Approved { .. }) => "approved",
        Some(ApprovalStatus::Denied { .. }) => "denied",
        Some(ApprovalStatus::UserApproval { .. }) => "user-approval",
        Some(_) => "unknown",
        None => "not-applicable",
    }
}
