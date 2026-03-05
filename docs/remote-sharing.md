---
title: Remote Sharing
description: Sync token savings across machines and view aggregate stats via the tokf server.
order: 5
---

## Setup

```sh
tokf auth login            # authenticate via GitHub device flow
tokf remote setup          # register this machine
tokf sync                  # upload pending usage events
tokf gain --remote         # view aggregate savings across all machines
```

## Authentication

tokf uses the [GitHub device flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow) so no secrets are handled locally. Tokens are stored in your OS keyring (Keychain on macOS, Secret Service on Linux, Credential Manager on Windows).

```sh
tokf auth login    # start device flow — prints a one-time code, opens browser
tokf auth status   # show current login state and server URL
tokf auth logout   # remove stored credentials
```

## Machine registration

Each machine gets a UUID that links usage events to a physical device. Registration is idempotent — running it again re-syncs the existing record.

```sh
tokf remote setup    # register this machine with the server
tokf remote status   # show local machine ID and hostname (no network call)
```

Machine config is stored in `~/.config/tokf/machine.toml` (or `$TOKF_HOME/machine.toml`).

## Syncing usage data

`tokf sync` uploads pending local usage events to the remote server. Events are deduplicated by cursor — re-syncing the same events is safe.

```sh
tokf sync              # upload pending events
tokf sync --status     # show last sync time and pending event count (no network call)
```

A file lock prevents concurrent syncs. Both `tokf auth login` and `tokf remote setup` must be completed before syncing.

## Viewing remote gain

View aggregate token savings across all your registered machines:

```sh
tokf gain --remote              # summary: total runs, tokens saved, reduction %
tokf gain --remote --by-filter  # breakdown by filter
tokf gain --remote --json       # machine-readable output
```

> **Note:** `--daily` is not available with `--remote`. Use local `tokf gain --daily` for day-by-day breakdowns.

## Backfill

Usage events recorded before hash-based tracking was added may be missing filter hashes. Backfill resolves them from currently installed filters:

```sh
tokf remote backfill             # update events with missing hashes
tokf remote backfill --no-cache  # skip binary config cache during discovery
```

Backfill runs locally — no network call required.

---

For discovering and installing community filters, see [Community Filters](#community-filters). To publish your own, see [Publishing Filters](#publishing-filters). For the full server API reference, see [`docs/reference/api.md`](docs/reference/api.md).
