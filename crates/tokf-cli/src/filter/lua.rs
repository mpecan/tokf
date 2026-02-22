use anyhow::Context as _;
use mlua::Lua;

use crate::config::types::ScriptConfig;

fn load_source(script_config: &ScriptConfig) -> anyhow::Result<String> {
    match (&script_config.file, &script_config.source) {
        (Some(file), _) => std::fs::read_to_string(file)
            .with_context(|| format!("lua_script: cannot read file '{file}'")),
        (None, Some(source)) => Ok(source.clone()),
        (None, None) => Err(anyhow::anyhow!("lua_script must set 'file' or 'source'")),
    }
}

/// Run a Luau filter script against pre-filtered command output.
///
/// Globals available to the script:
///   output: string    — combined output (after top-level skip/keep)
///   `exit_code`: integer — command exit code
///   args: table       — command arguments (1-indexed Lua array)
///
/// Return value:
///   string → replace output; nil/no-return → passthrough (output unchanged)
pub fn run_lua_script(
    script_config: &ScriptConfig,
    output: &str,
    exit_code: i32,
    args: &[String],
) -> anyhow::Result<Option<String>> {
    let source = load_source(script_config)?;

    let lua = Lua::new();

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
        .load(source.as_str())
        .eval()
        .context("lua_script execution")?;

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
    use crate::config::types::{ScriptConfig, ScriptLang};

    fn inline_script(source: &str) -> ScriptConfig {
        ScriptConfig {
            lang: ScriptLang::Luau,
            file: None,
            source: Some(source.to_string()),
        }
    }

    #[test]
    fn lua_returns_string_replaces_output() {
        let config = inline_script(r#"return "replaced""#);
        let result = run_lua_script(&config, "original", 0, &[]).unwrap();
        assert_eq!(result, Some("replaced".to_string()));
    }

    #[test]
    fn lua_returns_nil_passthrough() {
        let config = inline_script("return nil");
        let result = run_lua_script(&config, "original", 0, &[]).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn lua_output_global_available() {
        let config = inline_script("return output");
        let result = run_lua_script(&config, "hello world", 0, &[]).unwrap();
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn lua_exit_code_global_available() {
        let config = inline_script("return tostring(exit_code)");
        let result = run_lua_script(&config, "", 0, &[]).unwrap();
        assert_eq!(result, Some("0".to_string()));
    }

    #[test]
    fn lua_args_global_available() {
        let config = inline_script("return args[1]");
        let args = vec!["hello".to_string()];
        let result = run_lua_script(&config, "", 0, &args).unwrap();
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn lua_file_not_found_returns_err() {
        let config = ScriptConfig {
            lang: ScriptLang::Luau,
            file: Some("/nonexistent/path/script.luau".to_string()),
            source: None,
        };
        let result = run_lua_script(&config, "", 0, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn lua_invalid_syntax_returns_err() {
        let config = inline_script("not lua !!!");
        let result = run_lua_script(&config, "", 0, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn lua_os_blocked_by_sandbox() {
        let config = inline_script(r#"return os.execute("id")"#);
        let result = run_lua_script(&config, "", 0, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn lua_io_blocked_by_sandbox() {
        let config = inline_script("return io.read()");
        let result = run_lua_script(&config, "", 0, &[]);
        assert!(result.is_err());
    }
}
