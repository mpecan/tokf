# Implement Issue Workflow

You are orchestrating a Plan → Implement → Review → Remediate → PR cycle for a GitHub issue,
with support for stacked PRs when issues form a linear dependency chain.

## Input

The user will provide a GitHub issue number: $ARGUMENTS

## Phase 1: Load Context & Determine Stacking

1. Fetch the issue details: `gh api repos/mpecan/tokf/issues/<number>`
2. If the issue references a parent/milestone issue, read it to understand the full implementation sequence
3. Check that all dependencies (listed in the issue body) are complete — either closed/merged to main, or have an open PR in the current stack
4. Read memory files (MEMORY.md) for project conventions

### Stacking Decision

Determine whether this issue should **stack** on a previous branch or **start fresh from main**.

**Algorithm:**
1. Find this issue's position in any implementation sequence
2. Look at the **previous issue** in the sequence (by step number, not by dependency list)
3. If the previous issue has an **unmerged PR branch** that is an ancestor of this issue's dependencies → **stack on that branch**
4. If the previous issue is already **merged to main** → **branch from main**
5. If this issue has **multiple unmerged predecessors on different branches** → this is a **merge point**. STOP and tell the user the predecessor PRs must be merged first.

**In practice:**
```
# Check for unmerged predecessor branch
gh pr list --search "head:feat/" --json headRefName,state,title --jq '.[] | select(.state == "OPEN")'

# If stacking:
git checkout <predecessor-branch>
git checkout -b feat/<this-issue>

# If fresh from main:
git checkout main && git pull
git checkout -b feat/<this-issue>
```

Report the stacking decision to the user:
```
Stacking: feat/<this-issue> → feat/<predecessor-issue> → ... → main
PR will target: feat/<predecessor-issue>
```
or:
```
Fresh branch: feat/<this-issue> from main
PR will target: main
```

**IMPORTANT:** Record the stacking decision (base branch and PR target) — this information must be included in the plan so it survives context compression.

## Phase 2: Plan

1. Enter plan mode with `EnterPlanMode`
2. Explore the codebase areas relevant to the issue using Grep/Glob/Read
3. Read CLAUDE.md for project conventions (conventional commits, file limits, testing requirements)
4. Design the implementation approach:
   - Which files to create/modify
   - What types, traits, functions to add
   - How it integrates with existing code
   - What tests to write (TDD — tests first)

### Plan Content Requirements

The plan must be **self-contained** — Claude Code's plan mode keeps the plan file fully loaded after context compression, so the plan becomes the primary reference for all subsequent phases. Include ALL of the following sections:

#### Section 1: Branch & Stacking
```markdown
## Branch Strategy
- **Branch name:** `feat/<number>-<short-description>`
- **Base branch:** `main` | `feat/<predecessor-branch>`
- **PR target:** `main` | `feat/<predecessor-branch>`
- **Create command:** `git checkout <base> && git pull && git checkout -b feat/<branch-name>`
```

#### Section 2: Implementation Plan
The actual code changes — files, types, functions, integration points, estimated line counts, file size compliance checks.

**File size compliance:** For each file being created or modified, note:
- Current line count (for modified files)
- Estimated final line count
- Whether it stays under 500 (soft limit, CI warns) / 700 (hard limit, CI fails)

#### Section 3: Implementation Order
Numbered steps for the implementation, including:
1. Create the branch (with exact command from Section 1)
2. Implementation steps (code changes)
3. Quality gate steps

#### Section 4: Post-Implementation Checklist
This section ensures nothing is forgotten after the plan is accepted. Include it verbatim:

```markdown
## Post-Implementation Checklist

### Quality Gate (run all, fix any failures before review)
- [ ] `cargo fmt -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] `tokf verify` (if filter changes were made)

### Multi-Agent Review (4 parallel agents)
Launch 4 review agents in parallel using the Agent tool:
1. **Acceptance Criteria** — verify each criterion from the issue with PASS/FAIL
2. **Code Quality** — file/function size limits, duplication, naming, error handling
3. **Architecture** — module structure, API design, pattern consistency, downstream impact
4. **Test Coverage** — public API coverage, edge cases, meaningful assertions

Provide each agent with:
- The issue description and acceptance criteria
- The diff command: `git diff <base-branch>...HEAD`

### Remediation
- Fix all MAJOR findings, re-run quality gate
- Present MINOR findings to user for decision

### PR Creation
- Push: `git push -u origin <branch-name>`
- Create PR: `gh pr create --base <pr-target> --title "..." --body "..."`
- PR body must include: `Closes #<number>`, summary, test plan
- If stacked: include Stack section in PR body
- Wait for CI to pass (`gh pr checks <number> --watch`), fix failures if any
- Report PR URL and next issue in sequence (if any)
```

5. Write the plan using the plan mode tool (do NOT write to `.claude/plan.md` manually — the plan mode tool manages its own file that survives context compression)
6. Exit plan mode and wait for user approval

**STOP: Wait for user to approve the plan before proceeding.**

## Phase 3: Implement

1. **Create the branch** per the Branch Strategy section of the approved plan
2. Create tasks for each implementation step using TaskCreate
3. Follow TDD:
   - Write tests first
   - Run tests to confirm they fail
   - Implement the code
   - Run tests to confirm they pass
4. Follow project standards (from CLAUDE.md):
   - Files: soft limit 500 lines (CI warns), hard limit 700 lines (CI fails)
   - Functions: under 60 lines (clippy.toml enforced)
   - Conventional commits: `<type>(<scope>): <description>`
   - Scopes: config, filter, runner, output, cli, hook, tracking, history
   - Minimum 80% test coverage, target 90%
   - Declarative filter tests in `_test/` directories when adding/modifying filters
5. Mark tasks complete as you go

## Phase 4: Quality Gate

Run the automated quality gate:

```
cargo fmt -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

If filter changes were made, also run:
```
tokf verify
```

If any failures, fix them before proceeding to the multi-agent review.

## Phase 5: Multi-Agent Review

Spawn **four review agents in parallel** using the Agent tool. Each agent reviews **only this issue's changes** from a different perspective.

**Important for stacked PRs:** The diff must be scoped to only this issue's commits, not the entire stack.
```
# For stacked PRs (base is predecessor branch):
git diff <predecessor-branch>...HEAD

# For fresh branches (base is main):
git diff main...HEAD
```

All agents should be given the issue description, acceptance criteria, and the correct diff command.

**Important:** Agents should use `git diff` output to review changes, NOT read individual files directly. Reading files can produce stale results if edits happen between agent launch and file read. The diff is the source of truth for what changed.

### Agent 1: Acceptance Criteria Verification
**subagent_type:** `general-purpose`
```
Review the changes (using the provided diff command) against the issue's acceptance criteria.
For each criterion, state PASS or FAIL with evidence (file:line references).
Flag any acceptance criteria that are partially met or ambiguous.
```

### Agent 2: Code Quality & Standards
**subagent_type:** `general-purpose`
```
Review the changes (using the provided diff command) for:
- File length (soft 500 / hard 700 lines)
- Function length (under 60 lines, clippy enforced)
- Code duplication — check against existing patterns in the codebase
- Naming conventions consistent with surrounding code
- Error handling follows existing Result<T, E> / anyhow patterns
- No unnecessary pub visibility
Rate each file: CLEAN, MINOR (nitpicks), or MAJOR (must fix).
```

### Agent 3: Architecture & Integration
**subagent_type:** `general-purpose`
```
Review the changes (using the provided diff command) for:
- Does this integrate correctly with the existing module structure?
- Are new types/traits placed in the right crates (tokf-cli, tokf-common)?
- Are public API surfaces minimal and well-designed?
- Does it follow established patterns in the codebase?
- Are there any circular dependencies introduced?
- Will this cause problems for downstream work?
Rate: CLEAN, MINOR, or MAJOR.
```

### Agent 4: Test Coverage & Correctness
**subagent_type:** `general-purpose`
```
Review the changes (using the provided diff command) for:
- Are all new public functions/methods covered by tests?
- Are edge cases tested (empty input, errors, boundary conditions)?
- Do tests actually assert meaningful behavior (not just "doesn't panic")?
- Are integration tests present where needed?
- For filter changes: do declarative _test/ suites exist?
- Any test gaps that could hide bugs?
Rate: CLEAN, MINOR, or MAJOR.
```

### Synthesis

After all four agents complete, synthesize their findings into a structured report:

```markdown
## Review Summary

### Verdict: PASS / PASS WITH MINOR ITEMS / NEEDS REMEDIATION

### Acceptance Criteria: X/Y passed

### Findings by severity

#### MAJOR (must fix before PR)
- [ ] <finding> — <file:line> (from: <agent>)

#### MINOR (fix or consciously skip)
- [ ] <finding> — <file:line> (from: <agent>)

#### NOTES (informational)
- <observation> (from: <agent>)
```

## Phase 6: Remediate

If any MAJOR findings:
1. Fix each MAJOR item
2. Re-run the quality gate (Phase 4)
3. Re-run only the relevant review agents for changed areas
4. Update the review summary

For MINOR findings, present them to the user and let them decide which to address.

**STOP: Present the review summary to the user. Ask for confirmation before creating PR.**
Use `AskUserQuestion` with options:
- "Create PR as-is" — proceed to Phase 7
- "Fix minor items first" — address selected minor items, then re-review
- "I want to review the changes myself first" — pause, let user inspect

## Phase 7: PR

1. Push the branch: `git push -u origin <branch-name>`
2. Create the PR with the correct base branch:
   ```
   # Stacked PR:
   gh pr create --base feat/<predecessor-issue> --title "..." --body "..."

   # Fresh from main:
   gh pr create --base main --title "..." --body "..."
   ```
3. PR body format:
   - Title: conventional commit style matching the issue (e.g. `feat(hook): add Gemini CLI integration`)
   - Body must include:
     - `Closes #<number>`
     - Summary of changes and test plan
     - If stacked: a "Stack" section listing the chain:
       ```
       ## Stack
       - #<PR-N> ← **this PR**
       - #<PR-N-1> (base)
       - #<PR-N-2>
       - main
       ```
4. Report the PR URL to the user
5. Wait for CI checks to complete and report status:
   ```
   gh pr checks <pr-number> --watch
   ```
   - If CI passes, report success and move on
   - If CI fails, investigate the failure, fix the issue, push a new commit, and wait for CI again
   - Repeat until CI is green
6. If there is a next issue in the sequence that can be stacked, inform the user:
   ```
   Next in sequence: #<next-issue> — <title>
   This can be stacked on the branch just created. Run: /implement-issue <next-issue>
   ```

## Conventions

- Branch naming: `feat/<issue-number>-<short-description>` (e.g. `feat/283-ide-integrations`)
- Commit messages: conventional commits — `<type>(<scope>): <description>` (see CLAUDE.md for types and scopes)
- One commit per logical change, squash noise commits
- Always reference the issue number in commit messages
- **README.md is generated** — edit `docs/*.md` then run `bash scripts/generate-readme.sh`; the pre-commit hook does this automatically

## Stacking Reference

### When stacking works (linear chain)
```
main ← feat/283-ide ← feat/284-discover ← feat/285-generic
         PR #A (→main)  PR #B (→feat/283)   PR #C (→feat/284)
```
Each PR shows only its own diff. Merge from the bottom up: A first, then rebase B onto main, etc.

### When stacking stops (fan-out / merge point)
If two issues share a dependency but are on different branches, that's a merge point.
The shared dependency must be merged to main before either can proceed.

### After merging a stacked PR
When a PR at the base of a stack is merged to main:
1. The next PR in the stack needs its base updated to main
2. `gh pr edit <number> --base main`
3. Or rebase: `git rebase --onto main feat/<merged-branch> feat/<next-branch>`
