# tokf Terms of Service

**Version 1** — Effective 2026-03-02

## What tokf is

tokf is a free, open-source command-output filter for LLM coding assistants.
The optional server component (`api.tokf.net`) provides account management,
filter sharing, and aggregate usage statistics. tokf is a community project
with no profit motive.

## What data we collect

When you create an account and use tokf with the remote server, we store:

- **GitHub profile**: your username, avatar URL, profile URL, and organization
  memberships (public information from the GitHub API).
- **Machine identifiers**: a locally-generated UUID and hostname for each
  machine you register, used to deduplicate sync events.
- **Aggregate usage statistics**: per-filter token counts (input tokens,
  output tokens, command count). We do **not** store your command content,
  arguments, or output.
- **Published filters**: filter TOML files and test suites you choose to share
  with the community.
- **Terms of Service records**: which ToS version you accepted and when.

We do **not** collect or store:
- Command content or arguments
- Command output (raw or filtered)
- File contents or directory structures
- Environment variables or secrets

## What we use it for

- **Attribution**: linking published filters to their author.
- **Aggregate statistics**: computing community-wide token savings displayed
  on the website and in `tokf gain --remote`.
- **Sync deduplication**: ensuring usage events are not double-counted across
  machines.

We do not sell, share, or monetize your data. There is no advertising.

## No guarantees

tokf is provided **"as is"**, without warranty of any kind, express or implied.
The service is maintained by volunteers and may experience downtime, data loss,
or discontinuation without notice. We make no guarantees about availability,
accuracy, or fitness for any particular purpose.

## Account deletion

You can delete your account at any time by running:

```
tokf auth delete-account
```

Deletion takes effect **immediately**. When you delete your account:

- Your auth tokens, machine registrations, usage events, sync cursors,
  and ToS acceptance records are permanently removed.
- Your user profile is anonymized: personal details are cleared and the
  account is marked as deleted.
- Filters you published remain available to the community. Your account
  is converted to an anonymized, unclaimed state (similar to stdlib filter
  authors). Your GitHub username is no longer displayed.

## Changes to these terms

When we update these terms, the version number increases. You will be prompted
to review and accept the new version the next time you run `tokf auth login`.
Continued use of the remote server requires acceptance of the current terms.

## Contact

tokf is open source: <https://github.com/mpecan/tokf>

For questions or concerns about these terms, open an issue on the repository.
