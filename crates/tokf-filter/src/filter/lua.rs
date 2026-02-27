use anyhow::Context as _;
use mlua::Lua;

use tokf_common::config::types::ScriptConfig;

/// Default instruction limit for sandboxed execution (1 million instructions).
const DEFAULT_INSTRUCTION_LIMIT: u32 = 1_000_000;

/// Default memory limit for sandboxed execution (16 MB).
const DEFAULT_MEMORY_LIMIT: usize = 16 * 1024 * 1024;

/// Load the Lua script source from a [`ScriptConfig`].
///
/// Resolves `file` (read from disk) or `source` (inline). Setting both
/// is rejected — exactly one must be provided.
///
/// This is `pub(crate)` because callers should use
/// [`run_lua_script_sandboxed`] for execution; this function is only
/// exposed for the `apply` pipeline to load + run as two steps.
///
/// # Errors
///
/// Returns an error if both `file` and `source` are set, if neither is
/// set, or if the referenced file cannot be read.
pub(crate) fn load_source(script_config: &ScriptConfig) -> anyhow::Result<String> {
    match (&script_config.file, &script_config.source) {
        (Some(_), Some(_)) => Err(anyhow::anyhow!(
            "lua_script must set 'file' or 'source', not both"
        )),
        (Some(file), None) => std::fs::read_to_string(file)
            .with_context(|| format!("lua_script: cannot read file '{file}'")),
        (None, Some(source)) => Ok(source.clone()),
        (None, None) => Err(anyhow::anyhow!("lua_script must set 'file' or 'source'")),
    }
}

/// Sandboxed Lua execution limits.
pub struct SandboxLimits {
    /// Maximum number of Luau instructions before termination.
    pub instruction_limit: u32,
    /// Maximum memory in bytes the Luau VM may allocate.
    pub memory_limit: usize,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            instruction_limit: DEFAULT_INSTRUCTION_LIMIT,
            memory_limit: DEFAULT_MEMORY_LIMIT,
        }
    }
}

/// Run a Luau filter script with resource limits (instruction count + memory).
///
/// Prevents infinite loops and memory exhaustion via instruction-count
/// and memory-limit constraints. Only inline source code is accepted.
///
/// ## Sandbox guarantees
///
/// mlua's Luau VM disables `os`, `io`, `package`, and other dangerous
/// standard libraries by default — scripts cannot access the filesystem
/// or execute commands. This is verified by tests (`os_blocked_by_sandbox`,
/// `io_blocked_by_sandbox`). If upgrading mlua, re-run these tests to
/// confirm the sandbox is intact.
///
/// # Errors
///
/// Returns an error if:
/// - The script exceeds the instruction limit (likely infinite loop)
/// - The script exceeds the memory limit
/// - Any other Lua runtime error
pub fn run_lua_script_sandboxed(
    source: &str,
    output: &str,
    exit_code: i32,
    args: &[String],
    limits: &SandboxLimits,
) -> anyhow::Result<Option<String>> {
    // mlua's Luau VM sandboxes by default: os, io, package, etc. are nil.
    let lua = Lua::new();

    // Set memory limit (returns previous limit, which we discard)
    let _ = lua.set_memory_limit(limits.memory_limit);

    // Per-invocation counter — each call gets its own Arc so there's no
    // cross-call interference. The interrupt handler runs synchronously on
    // the VM's single thread, so Relaxed ordering is safe.
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let instruction_limit = limits.instruction_limit;
    lua.set_interrupt(move |_lua| {
        // mlua's Luau interrupt fires roughly every ~1000 VM instructions.
        // saturating_mul caps at u32::MAX which is always >= any reasonable
        // instruction_limit, so overflow doesn't bypass the check.
        let calls = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if calls.saturating_mul(1000) >= instruction_limit {
            return Ok(mlua::VmState::Yield);
        }
        Ok(mlua::VmState::Continue)
    });

    lua.globals()
        .set("output", output)
        .context("set output global")?;
    lua.globals()
        .set("exit_code", exit_code)
        .context("set exit_code global")?;

    let args_table = lua.create_table().context("create args table")?;
    for (i, arg) in args.iter().enumerate() {
        args_table
            .set(i + 1, arg.as_str())
            .with_context(|| format!("set args[{}]", i + 1))?;
    }
    lua.globals()
        .set("args", args_table)
        .context("set args global")?;

    let value: mlua::Value = lua
        .load(source)
        .eval()
        .context("sandboxed lua_script execution")?;

    match value {
        mlua::Value::String(s) => {
            let text = s.to_str()?.to_string();
            Ok(Some(text))
        }
        mlua::Value::Nil => Ok(None),
        other => Err(anyhow::anyhow!(
            "lua_script must return a string or nil, got {}",
            other.type_name()
        )),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tokf_common::config::types::{ScriptConfig, ScriptLang};

    /// Tiny limits for tests — just enough for the VM to init and run a
    /// simple return statement. The Luau VM needs ~512 KB for its own
    /// baseline allocations.
    fn test_limits() -> SandboxLimits {
        SandboxLimits {
            instruction_limit: 10_000,
            memory_limit: 512 * 1024, // 512 KB
        }
    }

    #[test]
    fn returns_string_replaces_output() {
        let result =
            run_lua_script_sandboxed(r#"return "replaced""#, "original", 0, &[], &test_limits())
                .unwrap();
        assert_eq!(result, Some("replaced".to_string()));
    }

    #[test]
    fn returns_nil_passthrough() {
        let result =
            run_lua_script_sandboxed("return nil", "original", 0, &[], &test_limits()).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn output_global_available() {
        let result =
            run_lua_script_sandboxed("return output", "hello world", 0, &[], &test_limits())
                .unwrap();
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn exit_code_global_available() {
        let result =
            run_lua_script_sandboxed("return tostring(exit_code)", "", 0, &[], &test_limits())
                .unwrap();
        assert_eq!(result, Some("0".to_string()));
    }

    #[test]
    fn args_global_available() {
        let args = vec!["hello".to_string()];
        let result =
            run_lua_script_sandboxed("return args[1]", "", 0, &args, &test_limits()).unwrap();
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn load_source_file_not_found_returns_err() {
        let config = ScriptConfig {
            lang: ScriptLang::Luau,
            file: Some("/nonexistent/path/script.luau".to_string()),
            source: None,
        };
        let result = load_source(&config);
        assert!(result.is_err());
    }

    #[test]
    fn load_source_rejects_both_file_and_source() {
        let config = ScriptConfig {
            lang: ScriptLang::Luau,
            file: Some("script.luau".to_string()),
            source: Some("return nil".to_string()),
        };
        let result = load_source(&config);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not both"),
            "expected mutual exclusion error, got: {msg}"
        );
    }

    #[test]
    fn load_source_rejects_neither_file_nor_source() {
        let config = ScriptConfig {
            lang: ScriptLang::Luau,
            file: None,
            source: None,
        };
        let result = load_source(&config);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_syntax_returns_err() {
        let result = run_lua_script_sandboxed("not lua !!!", "", 0, &[], &test_limits());
        assert!(result.is_err());
    }

    #[test]
    fn os_blocked_by_sandbox() {
        let result =
            run_lua_script_sandboxed(r#"return os.execute("id")"#, "", 0, &[], &test_limits());
        assert!(result.is_err());
    }

    #[test]
    fn io_blocked_by_sandbox() {
        let result = run_lua_script_sandboxed("return io.read()", "", 0, &[], &test_limits());
        assert!(result.is_err());
    }

    #[test]
    fn infinite_loop_returns_error() {
        let limits = SandboxLimits {
            instruction_limit: 1_000,
            memory_limit: 128 * 1024,
        };
        let result = run_lua_script_sandboxed("while true do end", "", 0, &[], &limits);
        assert!(result.is_err(), "infinite loop should be terminated");
    }

    #[test]
    #[ignore = "memory bomb test is slow — run with --ignored"]
    fn memory_bomb_returns_error() {
        let limits = SandboxLimits {
            instruction_limit: 500_000,
            memory_limit: 256 * 1024,
        };
        let script = r#"
            local s = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"  -- 128 bytes
            for i = 1, 30 do
                s = s .. s
            end
            return s
        "#;
        let result = run_lua_script_sandboxed(script, "", 0, &[], &limits);
        assert!(result.is_err(), "memory bomb should be terminated");
    }
}
