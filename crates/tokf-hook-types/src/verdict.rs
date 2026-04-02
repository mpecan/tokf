/// The decision component of a permission verdict.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PermissionDecision {
    /// No deny/ask rules matched — safe to auto-allow.
    Allow,
    /// A deny rule matched — block the command.
    Deny,
    /// An ask rule matched — prompt the user for confirmation.
    Ask,
}

/// Verdict from checking a command against permission rules.
///
/// Carries both a decision and an optional human-readable reason
/// that can be forwarded to the AI model.
#[derive(Debug, Clone)]
pub struct PermissionVerdict {
    pub decision: PermissionDecision,
    pub reason: Option<String>,
}

impl PermissionVerdict {
    /// Create an Allow verdict (no reason needed).
    pub const fn allow() -> Self {
        Self {
            decision: PermissionDecision::Allow,
            reason: None,
        }
    }

    /// Create a Deny verdict with an optional reason.
    pub const fn deny(reason: Option<String>) -> Self {
        Self {
            decision: PermissionDecision::Deny,
            reason,
        }
    }

    /// Create an Ask verdict with an optional reason.
    pub const fn ask(reason: Option<String>) -> Self {
        Self {
            decision: PermissionDecision::Ask,
            reason,
        }
    }
}

impl PartialEq for PermissionVerdict {
    fn eq(&self, other: &Self) -> bool {
        self.decision == other.decision && self.reason == other.reason
    }
}

impl Eq for PermissionVerdict {}

impl PermissionVerdict {
    /// Compare only the decision, ignoring the reason.
    pub fn decision_eq(&self, other: &Self) -> bool {
        self.decision == other.decision
    }
}
