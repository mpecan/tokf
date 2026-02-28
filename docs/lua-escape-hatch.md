---
title: Lua Escape Hatch
description: Use Luau scripts for filter logic that TOML can't express.
order: 3
---

For logic that TOML can't express — numeric math, multi-line lookahead, conditional branching — embed a [Luau](https://luau.org/) script:

```toml
command = "my-tool"

[lua_script]
lang = "luau"
source = '''
if exit_code == 0 then
    return "passed"
else
    return "FAILED: " .. output:match("Error: (.+)") or output
end
'''
```

Available globals: `output` (string), `exit_code` (integer — the underlying command's real exit code, unaffected by `--no-mask-exit-code`), `args` (table).
Return a string to replace output, or `nil` to fall through to the rest of the TOML pipeline.

### Sandbox

All Lua execution is sandboxed — both in the CLI and on the server:

- **Blocked libraries:** `io`, `os`, `package` — no filesystem or network access.
- **Instruction limit:** 1 million VM instructions (prevents infinite loops).
- **Memory limit:** 16 MB (prevents memory exhaustion).

Scripts that exceed these limits are terminated and treated as a passthrough (the TOML pipeline continues as if no Lua script was configured).

### External script files

For local development you can keep the script in a separate `.luau` file:

```toml
[lua_script]
lang = "luau"
file = "transform.luau"
```

Only one of `file` or `source` may be set — not both. When you run `tokf publish`, file references are automatically inlined (the file content is embedded as `source`) so the published filter is self-contained. The script file must reside within the filter's directory — path traversal (e.g. `../secret.txt`) is rejected.
