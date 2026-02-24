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
The sandbox blocks `io`, `os`, and `package` — no filesystem or network access from scripts.
