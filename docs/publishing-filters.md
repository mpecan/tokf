---
title: Publishing Filters
description: Share your filters with the tokf community registry.
order: 9
---

## Publishing a Filter

```sh
tokf publish <filter-name>
```

Publishes a local filter to the community registry under the MIT license. Authentication is required — run `tokf auth login` first.

### Requirements

- The filter must be a **user-level or project-local** filter (not a built-in). Use `tokf eject` first if needed.
- At least one **test file** must exist in the adjacent `_test/` directory. The server runs these tests against your filter before accepting the upload.
- You must accept the **MIT license** (prompted on first publish, remembered afterwards).

### What happens on publish

1. The filter TOML is read and validated.
2. If the filter uses `lua_script.file`, the referenced script is **automatically inlined** — its content is embedded as `lua_script.source` so the published filter is self-contained. The script file must reside within the filter's directory (path traversal is rejected).
3. A content hash is computed from the parsed config. This hash is the filter's permanent identity.
4. The filter and test files are uploaded. The server verifies tests pass before accepting.
5. On success, the registry URL is printed.

### Options

| Flag | Description |
|------|-------------|
| `--dry-run` | Preview what would be published without uploading |
| `--update-tests` | Replace the test suite for an already-published filter |

### Examples

```sh
tokf publish git/push                  # publish a filter
tokf publish git/push --dry-run        # preview only
tokf publish --update-tests git/push   # replace test suite
```

### Size limits

- Filter TOML: 64 KB max
- Total upload (filter + tests): 1 MB max

### Lua scripts in published filters

Published filters must use **inline `source`** for Lua scripts — `lua_script.file` is not supported on the server. The `tokf publish` command handles this automatically by reading the file and embedding its content. You don't need to change your filter.

All Lua scripts in published filters are executed in a sandbox with resource limits (1 million instructions, 16 MB memory) during server-side test verification.
