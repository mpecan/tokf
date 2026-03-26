/// Verdict from checking a command against permission rules.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PermissionVerdict {
    /// No deny/ask rules matched — safe to auto-allow.
    Allow,
    /// A deny rule matched — pass through to the tool's native deny handling.
    Deny,
    /// An ask rule matched — rewrite the command but let the tool prompt the user.
    Ask,
}
