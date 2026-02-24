---
name: tokf-run
description: Compress verbose CLI output with tokf before returning results. Activates for git, cargo, npm, docker, go, gradle, kubectl, and other supported commands.
---

# tokf — Token Output Filter

tokf compresses verbose command output so you receive concise, structured results instead of raw terminal dumps. It saves context tokens and makes output easier to reason about.

## Rule

When running a supported command, prefix it with `tokf run`:

```
tokf run <command>
```

**Examples:**

```sh
tokf run git status
tokf run cargo test
tokf run npm test
tokf run docker ps
```

## Supported commands

The following commands have built-in filters. Prefix these with `tokf run`:

- `git status`, `git diff`, `git log`, `git push`, `git add`, `git commit`, `git show`
- `cargo build`, `cargo test`, `cargo check`, `cargo clippy`, `cargo install`
- `npm test`, `npm run`, `yarn test`, `pnpm test`
- `docker build`, `docker compose`, `docker ps`, `docker images`
- `go build`, `go vet`
- `gradle build`, `gradle test`, `gradle dependencies`
- `gh pr view`, `gh pr list`, `gh pr checks`, `gh issue view`, `gh issue list`
- `kubectl get pods`
- `next build`
- `pnpm add`, `pnpm install`
- `prisma generate`
- `pytest`
- `tsc`
- `ls`

Commands not in this list pass through unchanged when prefixed with `tokf run`.

## Important rules

1. **Never double-prefix.** If a command already starts with `tokf run`, do not add it again.
2. **Arguments pass through.** Include all flags and arguments after the base command: `tokf run cargo test --release -- my_test`.
3. **Fail-safe.** If `tokf` is not installed or not on PATH, run the command without the prefix.
4. **Environment variables.** Place env vars before `tokf run`: `RUST_LOG=debug tokf run cargo test`.
5. **Pipes.** Do not add redundant filtering pipes (e.g. `| grep`, `| tail`, `| head`) after `tokf run` commands — tokf already compresses the output. Piping tokf's output for other purposes (e.g. `tokf run cargo test | wc -l`) is fine.
