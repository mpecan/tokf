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

filter search directories:
  [local] /home/user/project/.tokf/filters (not found)
  [user] /home/user/.config/tokf/filters (not found)
  [built-in] <embedded> (always available)

tracking database:
  TOKF_DB_PATH: (not set)
  path: /home/user/.local/share/tokf/tracking.db (exists)

filter cache:
  path: /home/user/.cache/tokf/manifest.bin (exists)

filters:
  local:    0
  user:     0
  built-in: 38
  total:    38
```

Override the tracking database path with the `TOKF_DB_PATH` environment variable:

```sh
TOKF_DB_PATH=/tmp/my-tracking.db tokf info
```

## Cache management

tokf caches the filter discovery index for faster startup. The cache rebuilds automatically when filters change, but you can manage it manually:

```sh
tokf cache info    # show cache location, size, and validity
tokf cache clear   # delete the cache, forcing a rebuild on next run
```
