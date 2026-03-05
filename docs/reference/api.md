# tokf-server API Reference

Full reference for the tokf remote server API. For deployment instructions, see [DEPLOY.md](../../DEPLOY.md).

---

## Authentication

**Bearer token** — most endpoints require `Authorization: Bearer <token>`. Tokens are 64 hex characters (90-day TTL) obtained via the GitHub device flow.

**Service token** — the `POST /api/filters/publish-stdlib` endpoint requires a bearer token belonging to a user with stdlib publisher privileges.

---

## Error format

All errors return JSON:

```json
{ "error": "descriptive message" }
```

**Status codes:** 400 (validation), 401 (missing/invalid/expired token), 403 (no permission), 404 (not found), 409 (conflict), 429 (rate limited), 500 (internal).

---

## Rate limiting

Rate-limited responses (429) include these headers:

| Header | Description |
|---|---|
| `retry-after` | Seconds until quota resets |
| `x-ratelimit-limit` | Max requests per window |
| `x-ratelimit-remaining` | Requests left in window |
| `x-ratelimit-reset` | Seconds until window resets |

Default limits (configurable via `RATE_LIMITS` env var):

| Scope | Per | Max | Window |
|---|---|---|---|
| Device flow | IP | 10 | 1 hour |
| Publish | user | 20 | 1 hour |
| Search (user) | user | 300 | 1 hour |
| Search (IP) | IP | 60 | 1 minute |
| Download (IP) | IP | 120 | 1 minute |
| Sync | machine | 60 | 1 hour |
| General | token | 300 | 1 minute |

---

## Endpoints

### Health

#### `GET /health`

Liveness probe. Always returns 200 if the process is running — no database query.

**Auth:** none

**Response (200):**
```json
{ "status": "ok", "version": "0.2.13" }
```

#### `GET /ready`

Readiness probe. Queries `_sqlx_migrations` to verify database connectivity.

**Auth:** none

**Response (200):**
```json
{ "status": "ok", "version": "0.2.13", "database": "ok" }
```

**Response (503):**
```json
{ "status": "degraded", "version": "0.2.13", "database": "error" }
```

---

### Authentication

#### `POST /api/auth/device`

Starts the GitHub device authorization flow. Rate-limited to 10 flows per IP per hour.

**Auth:** none

**Response (201):**
```json
{
  "device_code": "dc-abc123",
  "user_code": "ABCD-1234",
  "verification_uri": "https://github.com/login/device",
  "expires_in": 900,
  "interval": 5
}
```

#### `POST /api/auth/token`

Polls for a completed device authorization. The CLI calls this on an interval until the user has authorized.

**Auth:** none

**Request:**
```json
{ "device_code": "dc-abc123" }
```

**Response (200) — authorized:**
```json
{
  "access_token": "64-hex-char-bearer-token",
  "token_type": "bearer",
  "expires_in": 7776000,
  "user": {
    "id": 123,
    "username": "octocat",
    "avatar_url": "https://avatars.githubusercontent.com/u/1?v=4"
  }
}
```

**Response (200) — pending:**
```json
{ "error": "authorization_pending" }
```

If the client polls too quickly, the server responds with `"error": "slow_down"` and an `interval` field indicating the new polling interval.

Re-polling a completed device code is idempotent — a fresh token is issued.

---

### Machines

#### `POST /api/machines`

Register or update a machine. Idempotent: re-registering the same UUID updates the hostname. Per-user limit of 50 machines.

**Auth:** bearer token

**Request:**
```json
{
  "machine_id": "uuid-v4",
  "hostname": "laptop.local"
}
```

**Response (201 | 200):**
```json
{
  "machine_id": "uuid-v4",
  "hostname": "laptop.local",
  "created_at": "2025-01-15T10:30:00Z",
  "last_sync_at": null
}
```

**Errors:** 400 (invalid UUID/hostname), 409 (UUID owned by different user), 429 (machine limit reached)

#### `GET /api/machines`

List all machines for the authenticated user, newest first.

**Auth:** bearer token

**Response (200):**
```json
[
  {
    "machine_id": "uuid-v4",
    "hostname": "laptop.local",
    "created_at": "2025-01-15T10:30:00Z",
    "last_sync_at": "2025-01-16T08:00:00Z"
  }
]
```

---

### Filters

#### `POST /api/filters`

Publish a filter. Multipart/form-data upload.

**Auth:** bearer token

**Content-Type:** `multipart/form-data`

**Fields:**

| Field | Required | Description |
|---|---|---|
| `filter` | yes | TOML file bytes (max 64 KB) |
| `test:<filename>` | yes (at least 1) | Test file bytes (total upload max 1 MB) |
| `mit_license_accepted` | yes | Must be `"true"` |

The server computes the content hash, runs tests (10s timeout), then stores the filter.

**Response (201 | 200):**
```json
{
  "content_hash": "64-hex-sha256",
  "command_pattern": "git push",
  "author": "octocat",
  "registry_url": "https://registry.tokf.net/filters/abc123..."
}
```

**Errors:** 400 (invalid TOML, tests fail, `lua_script.file` used, license not accepted), 429

#### `GET /api/filters`

Search published filters. Results ranked by `savings_pct * (1 + ln(total_commands + 1))`. `savings_pct` is on a 0–100 scale (e.g. `75.5` means 75.5% reduction).

**Auth:** bearer token

**Query params:**

| Param | Default | Description |
|---|---|---|
| `q` | (empty) | Search string (max 200 chars, matched against command pattern) |
| `limit` | 20 | Results to return (clamped to 1–100) |

**Response (200):**
```json
[
  {
    "content_hash": "64-hex",
    "command_pattern": "git push",
    "author": "octocat",
    "savings_pct": 75.5,
    "total_commands": 1234,
    "created_at": "2025-01-15T10:30:00Z",
    "is_stdlib": false
  }
]
```

#### `GET /api/filters/{hash}`

Get metadata for a specific filter by content hash.

**Auth:** bearer token

**Response (200):**
```json
{
  "content_hash": "64-hex",
  "command_pattern": "git push",
  "author": "octocat",
  "savings_pct": 75.5,
  "total_commands": 1234,
  "created_at": "2025-01-15T10:30:00Z",
  "test_count": 5,
  "registry_url": "https://registry.tokf.net/filters/abc123...",
  "is_stdlib": false
}
```

**Errors:** 404

#### `GET /api/filters/{hash}/download`

Download a filter's TOML and test files.

**Auth:** bearer token

**Response (200):**
```json
{
  "filter_toml": "# full TOML content...",
  "test_files": [
    { "filename": "success.toml", "content": "..." },
    { "filename": "failure.toml", "content": "..." }
  ]
}
```

**Errors:** 404

#### `PUT /api/filters/{hash}/tests`

Replace the test suite for an already-published filter. Only the original author can update tests.

**Auth:** bearer token (must be original author)

**Content-Type:** `multipart/form-data`

**Fields:** `test:<filename>` (at least 1, total max 1 MB)

The server verifies tests pass (10s timeout) before committing. The old test suite is atomically replaced.

**Response (200):**
```json
{
  "content_hash": "64-hex",
  "command_pattern": "git push",
  "author": "octocat",
  "test_count": 7,
  "registry_url": "https://registry.tokf.net/filters/abc123..."
}
```

**Errors:** 400 (no tests, validation fails), 403 (not the author), 404

#### `POST /api/filters/publish-stdlib`

Publish standard library filters (marked `is_stdlib = true`). 5 MB body limit.

**Auth:** bearer token (stdlib publisher only)

---

### Sync

#### `POST /api/sync`

Upload usage events from a machine. Events with `id <= cursor` are skipped (idempotent). Returns the new cursor for the next sync.

**Auth:** bearer token

**Request:**
```json
{
  "machine_id": "uuid-v4",
  "last_event_id": 12345,
  "events": [
    {
      "id": 12346,
      "filter_name": "git/push",
      "filter_hash": "64-hex-or-null",
      "input_tokens": 1000,
      "output_tokens": 150,
      "command_count": 5,
      "recorded_at": "2025-01-15T10:30:00Z"
    }
  ]
}
```

**Constraints:**

| Field | Limit |
|---|---|
| Batch size | max 1000 events |
| Tokens per event | 0–10,000,000 |
| Command count | 0–100,000 |
| Filter name | max 1024 chars |
| Timestamps | RFC 3339 format |

**Response (200):**
```json
{
  "accepted": 3,
  "cursor": 12348
}
```

**Errors:** 400 (batch too large, invalid values), 404 (machine not found), 429

---

### Gain

#### `GET /api/gain`

Aggregate token savings for the authenticated user across all machines.

**Auth:** bearer token

**Response (200):**
```json
{
  "total_input_tokens": 50000,
  "total_output_tokens": 5000,
  "total_commands": 100,
  "by_machine": [
    {
      "machine_id": "uuid",
      "hostname": "laptop.local",
      "total_input_tokens": 30000,
      "total_output_tokens": 3000,
      "total_commands": 50
    }
  ],
  "by_filter": [
    {
      "filter_name": "git/push",
      "filter_hash": "64-hex-or-null",
      "total_input_tokens": 20000,
      "total_output_tokens": 2000,
      "total_commands": 40
    }
  ]
}
```

`filter_name` and `filter_hash` in `by_filter` entries may be `null` for events recorded before hash-based tracking.

#### `GET /api/gain/global`

Aggregate token savings across all users. Public endpoint. Hostnames are omitted for privacy. Results capped at top 100 machines and 100 filters.

**Auth:** none

**Response (200):**
```json
{
  "total_input_tokens": 5000000,
  "total_output_tokens": 750000,
  "total_commands": 50000,
  "by_machine": [
    {
      "machine_id": "uuid",
      "total_input_tokens": 1000000,
      "total_output_tokens": 150000,
      "total_commands": 5000
    }
  ],
  "by_filter": [
    {
      "filter_name": "git/push",
      "filter_hash": "64-hex",
      "total_input_tokens": 2000000,
      "total_output_tokens": 300000,
      "total_commands": 10000
    }
  ]
}
```

#### `GET /api/gain/filter/{hash}`

Token savings for a specific filter. Public endpoint.

**Auth:** none

**Response (200):**
```json
{
  "filter_hash": "64-hex",
  "command_pattern": "git push",
  "total_commands": 5000,
  "total_input_tokens": 1000000,
  "total_output_tokens": 150000,
  "savings_pct": 85.0,
  "last_updated": "2025-01-16T08:00:00Z"
}
```

`command_pattern` may be `null` if the filter metadata is unavailable.

**Errors:** 404

---

## Environment variables

Server configuration:

| Variable | Required | Description |
|---|---|---|
| `DATABASE_URL` | yes | PostgreSQL connection string |
| `GITHUB_CLIENT_ID` | yes | OAuth App client ID |
| `GITHUB_CLIENT_SECRET` | yes | OAuth App client secret |
| `PUBLIC_URL` | no | Base URL for registry links (default: `http://localhost:8080`) |
| `TRUST_PROXY` | no | Trust `X-Forwarded-For` for IP extraction (default: `false`) |
| `RUN_MIGRATIONS` | no | Run migrations on startup (default: `true`) |
| `RATE_LIMITS` | no | JSON override for rate limit configuration |
| `PORT` | no | Listen port (default: `8080`) |
| `MIGRATION_DATABASE_URL` | no | Separate connection string for running migrations (allows elevated DDL privileges) |
| `R2_ACCOUNT_ID` | no | Cloudflare account ID (used to derive R2 endpoint when `R2_ENDPOINT` is not set) |
| `R2_ENDPOINT` | no | S3/R2 endpoint for filter storage |
| `R2_ACCESS_KEY_ID` | no | S3/R2 access key |
| `R2_SECRET_ACCESS_KEY` | no | S3/R2 secret key |
| `R2_BUCKET_NAME` | no | S3/R2 bucket name |

See [DEPLOY.md](../../DEPLOY.md) for full deployment instructions.
