---
title: Diagnostics
description: Inspect your tokf setup, manage the filter cache, and troubleshoot.
order: 7
---

## tokf info

`tokf info` prints a summary of all paths, database locations, and filter counts. Useful for debugging when filters aren't being found or to verify your setup:

```sh
tokf info          # human-readable output
tokf info --json   # machine-readable JSON
```

Example output:

```
tokf 0.2.8
TOKF_HOME: (not set)

filter search directories:
  [local] /home/user/project/.tokf/filters (not found)
  [user] /home/user/.config/tokf/filters (not found)
  [built-in] <embedded> (always available)

tracking database:
  TOKF_DB_PATH: (not set)
  path: /home/user/.local/share/tokf/tracking.db (will be created)

filter cache:
  path: /home/user/.cache/tokf/manifest.bin (will be created)

filters:
  local:    0
  user:     0
  built-in: 38
  total:    38
```

### Environment variables

| Variable | Description | Default |
|---|---|---|
| `TOKF_HOME` | Redirect **all** user-level tokf paths (filters, cache, DB, hooks, auth) to a single directory | Platform config dir (e.g. `~/.config/tokf` on Linux) |
| `TOKF_DB_PATH` | Override the tracking database path only (takes precedence over `TOKF_HOME`) | Platform data dir (e.g. `~/.local/share/tokf/tracking.db`); or `$TOKF_HOME/tracking.db` when `TOKF_HOME` is set |
| `TOKF_NO_FILTER` | Skip filtering in shell mode (set to `1`, `true`, or `yes`) | unset |
| `TOKF_VERBOSE` | Print filter resolution details in shell mode | unset |

`TOKF_HOME` works like `CARGO_HOME` or `RUSTUP_HOME` — set it once to relocate everything:

```sh
# Put all tokf data under /opt/tokf (useful on read-only home dirs or shared systems)
TOKF_HOME=/opt/tokf tokf info

# Override only the tracking database, leave everything else in the default location
TOKF_DB_PATH=/tmp/my-tracking.db tokf info
```

The `tokf info` output always shows the active `TOKF_HOME` value (or `(not set)`) at the top,
so you can quickly verify which paths are in effect.

## Rewrite debugging

Use `tokf rewrite --verbose` to see how a command would be rewritten, including which rule fired:

```sh
tokf rewrite --verbose "make check"         # shows wrapper rule
tokf rewrite --verbose "cargo test"          # shows filter rule
tokf rewrite --verbose "cargo test | tail"   # shows pipe stripping
```

For shell mode (task runner recipe lines), set `TOKF_VERBOSE=1` to see filter resolution for each recipe line:

```sh
TOKF_VERBOSE=1 make check    # verbose output on stderr for each recipe
```

## Cache management

tokf caches the filter discovery index for faster startup. The cache rebuilds automatically when filters change, but you can manage it manually:

```sh
tokf cache info    # show cache location, size, and validity
tokf cache clear   # delete the cache, forcing a rebuild on next run
```
