---
title: Community Filters
description: Discover, install, and share filters with the tokf community registry.
order: 8
---

## Overview

The tokf community registry lets you discover filters published by other users, install them
locally, and share your own filters with the community.

Authentication is required for all registry operations. Run `tokf auth login` first.

---

## Searching for Filters

```sh
tokf search <query>
```

Returns filters whose command pattern matches `<query>` as a substring, ranked by token savings
and install count.

```
COMMAND              AUTHOR    SAVINGS%   INSTALLS
git push             alice       42.3%     1,234
git push --force     bob         38.1%       891
```

### Options

| Flag | Description |
|------|-------------|
| `-n, --limit <N>` | Maximum results to return (default: 20, max: 100) |
| `--json` | Output raw JSON array |

### Examples

```sh
tokf search git              # find all git filters
tokf search "cargo test"     # find cargo test filters
tokf search "" -n 50         # list 50 most popular filters
tokf search git --json       # machine-readable output
```

---

## Installing a Filter

```sh
tokf install <filter>
```

`<filter>` can be:

- A **command pattern** substring — tokf searches the registry and installs the top match.
- A **content hash** (64 hex characters) — installs a specific, pinned version.

On install, tokf:

1. Downloads the filter TOML and any bundled test files.
2. Verifies the content hash to detect tampering.
3. Writes the filter under `~/.config/tokf/filters/` (global) or `.tokf/filters/` (local).
4. Runs the bundled test suite (if any). Rolls back on failure.

### Options

| Flag | Description |
|------|-------------|
| `--local` | Install to project-local `.tokf/filters/` instead of global config |
| `--force` | Overwrite an existing filter at the same path |
| `--dry-run` | Preview what would be installed without writing any files |

### Examples

```sh
tokf install git push                  # install top result for "git push"
tokf install git push --local          # install into current project only
tokf install git push --dry-run        # preview the install
tokf install <64-hex-hash> --force     # install a pinned version, overwriting existing
```

### Attribution

Installed filters include an attribution header at the top of the TOML:

```toml
# Published by @alice · hash: <hash> · https://tokf.net/filters/<hash>
```

This header is stripped automatically when the filter is loaded.

### Security

> **Warning:** Community filters are third-party code. Review a filter at
> `https://tokf.net/filters/<hash>` before installing it in production environments.

tokf verifies the content hash of every downloaded filter to detect server-side tampering.
Test filenames are validated to prevent path traversal attacks.

---

## Updating Test Suites

After publishing a filter, the filter TOML itself is immutable (same content = same hash), but you
can replace the bundled test suite at any time:

```sh
tokf publish --update-tests <filter-name>
```

This replaces the **entire** test suite in the registry with the current local `_test/` directory
contents. Only the original author can update tests.

### Options

| Flag | Description |
|------|-------------|
| `--dry-run` | Preview which test files would be uploaded without making changes |

### Examples

```sh
tokf publish --update-tests git/push            # replace test suite for git/push
tokf publish --update-tests git/push --dry-run  # preview only
```

### Notes

- The filter's identity (content hash) does not change.
- The old test suite is deleted and fully replaced by the new one.
- You must be the original author of the filter.

---

## Publishing a Filter

See [Publishing Filters](./publishing-filters.md) for how to share your own filters with the community registry.
