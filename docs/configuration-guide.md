---
title: Configuration Guide
description: Central reference for all tokf configuration — files, environment variables, CLI flags, and resolution order.
order: 3
---

## Configuration files

tokf uses TOML files for configuration. Files can live at two levels:

| File | Global path | Project-local path | Purpose |
|------|-------------|---------------------|---------|
| `config.toml` | `~/.config/tokf/config.toml` | `.tokf/config.toml` | History retention, sync settings, telemetry |
| `rewrites.toml` | `~/.config/tokf/rewrites.toml` | `.tokf/rewrites.toml` | Shell rewrite rules |
| `auth.toml` | `~/.config/tokf/auth.toml` | — | Registry authentication (managed by `tokf auth`) |
| `machine.toml` | `~/.config/tokf/machine.toml` | — | Machine UUID for remote sync |

> Paths shown are for Linux/macOS. On macOS, the global directory is `~/Library/Application Support/tokf`. On Windows, it is `%APPDATA%/tokf`.

Set `TOKF_HOME` to redirect **all** user-level paths to a single directory (like `CARGO_HOME` or `RUSTUP_HOME`):

```sh
export TOKF_HOME=/opt/tokf   # all config, data, and cache under /opt/tokf
```

Run `tokf info` to see which paths are active.

---

## config.toml

### `[history]`

Controls how many filtered outputs are retained in the local history database.

```toml
[history]
retention = 10   # number of history entries to keep (default: 10)
```

### `[sync]`

Settings for remote filter-usage sync (requires `tokf auth login`).

```toml
[sync]
auto_sync_threshold = 100   # sync after this many unsynced records (default: 100)
upload_usage_stats = true   # upload anonymous usage statistics (default: not set)
```

### `[telemetry]`

Export metrics via OpenTelemetry OTLP. Disabled by default.

```toml
[telemetry]
enabled = true
endpoint = "http://localhost:4318"   # OTLP collector endpoint
protocol = "http"                    # "http" (default) or "grpc"
service_name = "tokf"                # service.name resource attribute

[telemetry.headers]
x-api-key = "your-secret"
```

Default endpoints by protocol: HTTP uses port `4318`, gRPC uses port `4317`.

Environment variables override config-file values for telemetry — see the [Environment variables](#environment-variables) section below.

### Priority order

For all `config.toml` settings:

1. **Project-local** `.tokf/config.toml` (highest priority)
2. **Global** `~/.config/tokf/config.toml`
3. **Built-in defaults**

### Managing config via CLI

```sh
tokf config show              # show all effective config with source paths
tokf config show --json       # machine-readable JSON output
tokf config get <key>         # print a single value (for scripting)
tokf config set <key> <value> # set a value in the global config
tokf config set --local <key> <value>  # set in project-local .tokf/config.toml
tokf config print             # print raw config file contents
tokf config path              # show config file paths with existence status
```

Available keys: `history.retention`, `sync.auto_sync_threshold`, `sync.upload_stats`.

---

## rewrites.toml

Rewrite rules let tokf intercept and transform commands before execution — wrapping task runners, stripping pipes, and injecting baselines. See the [Rewrite Configuration](#rewrite-configuration-rewritestoml) section for the full reference.

---

## Environment variables

| Variable | Description | Default |
|----------|-------------|---------|
| **Paths** | | |
| `TOKF_HOME` | Redirect all user-level tokf paths (config, data, cache) to a single directory | Platform config dir |
| `TOKF_DB_PATH` | Override the tracking database path only (takes precedence over `TOKF_HOME`) | Platform data dir |
| **Runtime** | | |
| `TOKF_DEBUG` | Enable debug output (set to `1` or `true`) | unset |
| `TOKF_NO_FILTER` | Skip filtering in shell mode (set to `1`, `true`, or `yes`) | unset |
| `TOKF_VERBOSE` | Print filter resolution details in shell mode | unset |
| `TOKF_PRESERVE_COLOR` | Preserve ANSI color codes in filtered output | unset |
| `TOKF_HTTP_TIMEOUT` | HTTP request timeout in seconds (for remote operations) | `5` |
| `NO_COLOR` | Disable colored output in `tokf gain` (per [no-color.org](https://no-color.org/)) | unset |
| **Telemetry** | | |
| `TOKF_TELEMETRY_ENABLED` | Enable telemetry export (`true`, `1`, or `yes`) — overrides config file | unset |
| `TOKF_OTEL_PIPELINE` | Pipeline label attached to telemetry metrics | unset |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP collector endpoint — overrides config file | `http://localhost:4318` |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `http` or `grpc` — overrides config file | `http` |
| `OTEL_EXPORTER_OTLP_HEADERS` | Comma-separated `key=value` header pairs — overrides config file | empty |
| `OTEL_RESOURCE_ATTRIBUTES` | Comma-separated `key=value` resource attributes; `service.name` is extracted | empty |
| **Registry (CI)** | | |
| `TOKF_REGISTRY_URL` | Registry base URL for `tokf regenerate-examples` | — |
| `TOKF_SERVICE_TOKEN` | Service token for registry authentication | — |

---

## CLI flags

### Global flags

Available on all `tokf run` invocations and most subcommands:

| Flag | Description |
|------|-------------|
| `--timing` | Print how long filtering took |
| `--verbose` | Show filter resolution details |
| `--no-filter` | Pass output through without filtering |
| `--no-cache` | Bypass the binary filter discovery cache |
| `--no-mask-exit-code` | Propagate real exit code instead of masking to 0 |
| `--preserve-color` | Preserve ANSI color codes in filtered output |
| `--otel-export` | Export metrics via OpenTelemetry OTLP for this invocation |

### Run-specific flags

| Flag | Description |
|------|-------------|
| `--baseline-pipe <cmd>` | Pipe command for fair baseline accounting (injected by rewrite rules) |
| `--prefer-less` | Compare filtered vs piped output and use whichever is smaller |

---

## Filter resolution order

tokf searches for matching filters in three tiers, stopping at the first match:

1. **Project-local** — `.tokf/filters/` in the project root
2. **User-level** — `~/.config/tokf/filters/` (or `$TOKF_HOME/filters/`)
3. **Standard library** — built-in filters shipped with tokf

To override a built-in filter, eject it to your project or user directory:

```sh
tokf eject cargo/test              # copy to .tokf/filters/ (project-local)
tokf eject --global cargo/test     # copy to ~/.config/tokf/filters/ (user-level)
```

The ejected copy takes priority on subsequent runs. See `tokf eject --help` for details.

---

## Directory layout

```
~/.config/tokf/                    # global config directory ($TOKF_HOME overrides)
├── config.toml                    # history, sync, telemetry settings
├── rewrites.toml                  # shell rewrite rules
├── auth.toml                      # registry credentials (managed by tokf auth)
├── machine.toml                   # machine UUID for remote sync
└── filters/                       # user-level filter overrides
    └── cargo/
        └── test.toml

~/.local/share/tokf/               # data directory
└── tracking.db                    # token savings database ($TOKF_DB_PATH overrides)

~/.cache/tokf/                     # cache directory
└── manifest.bin                   # binary filter discovery cache

<project>/
└── .tokf/                         # project-local overrides
    ├── config.toml                # project-specific settings
    ├── rewrites.toml              # project-specific rewrite rules
    └── filters/                   # project-specific filters
        └── custom/
            └── lint.toml
```
