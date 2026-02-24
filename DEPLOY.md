# Deploying tokf-server

This guide covers provisioning the external services and deploying
`tokf-server` to [Fly.io](https://fly.io).

## Prerequisites

- [flyctl](https://fly.io/docs/flyctl/install/) installed and authenticated
- A GitHub account with repository admin access (for Actions secrets)

## 1. CockroachDB Serverless

1. Create a free cluster at <https://cockroachlabs.cloud>.
2. Choose the **Serverless** plan and the region closest to US East
   (Ashburn, Virginia) to match the Fly.io `iad` primary region.
3. Create a SQL user and copy the connection string — it looks like:
   ```
   postgresql://<user>:<password>@<host>:26257/defaultdb?sslmode=verify-full
   ```
4. You will set this as the `DATABASE_URL` secret in Fly.io.

## 2. Cloudflare R2

R2 storage is optional. If no R2 variables are set, the server starts with
no-op storage (uploads are discarded). To enable blob storage:

1. In the Cloudflare dashboard, go to **R2 Object Storage** and create a bucket
   (e.g. `tokf-filters`).
2. Under **Manage R2 API Tokens**, create a token with **Object Read & Write**
   permission scoped to the bucket.
3. Note the **Access Key ID**, **Secret Access Key**, and your **Account ID**
   (found in the Cloudflare dashboard URL: `dash.cloudflare.com/<account-id>/...`).

If any R2 variable is set, all of them must be set — partial configuration is
an error.

## 3. GitHub OAuth App (Device Flow)

tokf uses the [GitHub device authorization flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow)
— there is no callback URL involved.

1. Go to **Settings > Developer settings > OAuth Apps > New OAuth App**.
2. Set the **Authorization callback URL** to any valid URL (e.g.
   `https://tokf.net`) — GitHub requires the field but it is never called in
   the device flow.
3. Under the app settings, ensure **Enable Device Flow** is checked.
4. Note the **Client ID** and generate a **Client Secret**.

## 4. Fly.io Setup

### Create the app

```sh
fly apps create tokf-server
```

### Set secrets

```sh
fly secrets set \
  DATABASE_URL="postgresql://..." \
  R2_BUCKET_NAME="tokf-filters" \
  R2_ACCESS_KEY_ID="..." \
  R2_SECRET_ACCESS_KEY="..." \
  R2_ACCOUNT_ID="..." \
  GITHUB_CLIENT_ID="..." \
  GITHUB_CLIENT_SECRET="..."
```

### Custom domain

```sh
fly certs add api.tokf.net
```

Then add a CNAME record pointing `api.tokf.net` to `tokf-server.fly.dev` in
your DNS provider.

### Deploy manually

```sh
fly deploy --remote-only
```

### CI deployment

The `.github/workflows/deploy-server.yml` workflow deploys automatically on
pushes to `main` that touch server code. It requires a `FLY_API_TOKEN` secret
in the repository:

```sh
# Create a deploy token scoped to the tokf-server app
fly tokens create deploy -x 87600h

# Add to GitHub repo secrets as FLY_API_TOKEN
```

## 5. Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DATABASE_URL` | Yes | — | PostgreSQL connection string |
| `PORT` | No | `8080` | HTTP listen port |
| `RUN_MIGRATIONS` | No | `true` | Run migrations on startup (`false` in fly.toml — handled by release_command) |
| `TRUST_PROXY` | No | `false` | Trust `X-Forwarded-For` headers (`true` in fly.toml) |
| `R2_BUCKET_NAME` | Conditional | — | Cloudflare R2 bucket name |
| `R2_ACCESS_KEY_ID` | Conditional | — | R2 API token access key |
| `R2_SECRET_ACCESS_KEY` | Conditional | — | R2 API token secret key |
| `R2_ACCOUNT_ID` | Conditional | — | Cloudflare account ID (or set `R2_ENDPOINT` directly) |
| `R2_ENDPOINT` | No | Derived from account ID | Explicit S3-compatible endpoint URL |
| `GITHUB_CLIENT_ID` | Yes | — | GitHub OAuth app client ID |
| `GITHUB_CLIENT_SECRET` | Yes | — | GitHub OAuth app client secret |
| `RUST_LOG` | No | `tokf_server=info,tower_http=info` | Log level filter (standard `tracing` / `env_filter` syntax) |

**R2 variables:** Either set all of `R2_BUCKET_NAME`, `R2_ACCESS_KEY_ID`,
`R2_SECRET_ACCESS_KEY`, and one of `R2_ACCOUNT_ID` / `R2_ENDPOINT` — or set
none (falls back to no-op storage).

## 6. Staging Environment

Create a separate Fly app for staging:

```sh
fly apps create tokf-server-staging
```

Set its own secrets (pointing to a separate CockroachDB database and R2 bucket):

```sh
fly secrets set --app tokf-server-staging \
  DATABASE_URL="postgresql://..." \
  GITHUB_CLIENT_ID="..." \
  GITHUB_CLIENT_SECRET="..."
```

Deploy with:

```sh
fly deploy --remote-only --app tokf-server-staging
```

For CI, you can add a separate workflow or use `workflow_dispatch` to deploy
to staging manually.

## 7. Health Checks

| Endpoint | Purpose | Behavior |
|----------|---------|----------|
| `GET /health` | Liveness | Always returns `200 OK` (no DB query) |
| `GET /ready` | Readiness | Queries `_sqlx_migrations` table; returns `200` if DB is reachable |

Fly.io is configured to check `/ready` every 10 seconds (see `fly.toml`).
This ensures traffic is only routed to machines with a working database
connection.

## 8. Migration Strategy

Migrations run automatically via Fly.io's `release_command` before the new
version receives traffic:

1. Fly builds the Docker image with the remote builder.
2. Before swapping traffic, Fly runs `./tokf-server migrate`.
3. If the migration fails, the deploy is aborted and the old version keeps
   running.
4. On success, traffic switches to the new machines.

The `RUN_MIGRATIONS` env var is set to `false` in `fly.toml` so the server
process itself does not re-run migrations on startup.
