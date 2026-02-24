## Server authentication API

tokf-server uses the [GitHub device flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow) so CLI clients can authenticate without handling secrets.

### `POST /api/auth/device`

Starts the device authorization flow. Returns a `user_code` and `verification_uri` for the user to visit in their browser. Rate-limited to 10 requests per IP per hour.

**Response (201 Created):**

```json
{
  "device_code": "dc-abc123",
  "user_code": "ABCD-1234",
  "verification_uri": "https://github.com/login/device",
  "expires_in": 900,
  "interval": 5
}
```

### `POST /api/auth/token`

Polls for a completed device authorization. The CLI calls this on an interval until the user has authorized.

**Request body:**

```json
{ "device_code": "dc-abc123" }
```

**Response (200 OK) when authorized:**

```json
{
  "access_token": "...",
  "token_type": "bearer",
  "expires_in": 7776000,
  "user": { "id": 1, "username": "octocat", "avatar_url": "..." }
}
```

**Response (200 OK) while waiting:**

```json
{ "error": "authorization_pending" }
```

Re-polling a completed device code is idempotent — a fresh token is issued.

### Environment variables

| Variable | Required | Description |
|---|---|---|
| `GITHUB_CLIENT_ID` | yes | OAuth App client ID |
| `GITHUB_CLIENT_SECRET` | yes | OAuth App client secret |
| `TRUST_PROXY` | no | Set `true` to trust `X-Forwarded-For` for IP extraction (default `false`) |

---

## Acknowledgements

tokf was heavily inspired by [rtk](https://github.com/rtk-ai/rtk) ([rtk-ai.app](https://www.rtk-ai.app/)) — a CLI proxy that compresses command output before it reaches an AI agent's context window. rtk pioneered the idea and demonstrated that 60–90% context reduction is achievable across common dev tools. tokf takes a different approach (TOML-driven filters, user-overridable library, Claude Code hook integration) but the core insight is theirs.

---

## License

MIT — see [LICENSE](LICENSE).
