//! Detection of "transparent-arg" commands — commands whose last argument
//! is opaque shell code that runs in a different environment (typically a
//! remote host or container shell). For these, tokf is allowed to prefix
//! the command (e.g. `tokf run ssh …`) but **must not** modify the argv
//! via regex `[[rewrite]]` rules — those can splice text into the opaque
//! payload and break the remote invocation. See issue #338.

/// Commands always treated as transparent. Extended (not replaced) by the
/// user's `[transparent] commands = […]` config.
///
/// Picked because each takes a remote shell-command as its last argument:
/// - `ssh`, `slogin` — OpenSSH and the legacy alias.
/// - `mosh` — wraps ssh, same model from a rewrite-safety POV.
///
/// `scp` / `rsync` are intentionally excluded: their last positional
/// argument is a file path, not shell code, so the regex-mangling failure
/// mode doesn't apply. `telnet` is excluded for the same reason (port
/// number, not a shell command).
pub const BUILTIN_TRANSPARENT_COMMANDS: &[&str] = &["ssh", "mosh", "slogin"];

/// Return true if the first command word's basename matches the built-in
/// transparent list or one of `user_extras`.
///
/// `user_extras` should be the `commands` field from
/// [`tokf_hook_types::TransparentConfig`] — basename matches, e.g.
/// `"kubectl"` matches both `kubectl` and `/usr/local/bin/kubectl`.
pub fn is_transparent_command(command: &str, user_extras: &[String]) -> bool {
    let Some(name) = super::bash_ast::first_command_basename(command) else {
        return false;
    };
    BUILTIN_TRANSPARENT_COMMANDS.iter().any(|b| *b == name)
        || user_extras.iter().any(|u| u == &name)
}

/// Return true if **any** segment of a compound command is a transparent-arg
/// invocation. Used to gate user `[[rewrite]]` regex rules at the top level
/// of `rewrite_with_config_and_options`: those rules run on the *whole*
/// command string, so even one ssh segment hidden behind a `cd … &&` is
/// enough to make it unsafe to splice text anywhere in the command.
pub fn any_segment_is_transparent(command: &str, user_extras: &[String]) -> bool {
    super::bash_ast::split_compound(command)
        .iter()
        .map(|(seg, _sep)| seg.trim())
        .any(|seg| is_transparent_command(seg, user_extras))
}
