# Git Tool Reference

Rune's `git` tool wraps a constrained subset of git and adds workflow-aware synthetic operations so agents can reason about shipping state without brittle shell pipelines.

## Supported operations

### Native git passthrough
- `status`
- `diff`
- `add`
- `commit`
- `push`
- `pull`
- `log`
- `branch`
- `checkout`
- `merge`

### Synthetic workflow operations
- `pr_status` — inspect whether the current or named branch already has a PR, plus merge/check status
- `staged` — inspect staged files and optionally enforce staged-content expectations
- `suggest_branch` — generate a normalized branch name from issue/task metadata

## Safety rails

Mutating operations (`add`, `commit`, `push`, `pull`, `checkout`, `merge`) support `safety` guards:

- `allow_dirty` — bypass dirty-tree blocking when intentional
- `require_clean` — require a clean tree before execution
- `require_base_in_sync` — fetch and verify HEAD is based on the latest remote base branch
- `base_branch` — remote base branch to compare against, default `main`
- `remote` — remote name, default `origin`
- `protected_branches` — branch names where mutating operations are blocked, default `main`, `master`

## `pr_status`

`pr_status` uses `gh pr view` and returns structured JSON rather than free-form CLI text.

### Inputs
- `branch` (optional) — branch to inspect; defaults to the current branch

### Output fields
- `has_pr`
- `number`
- `title`
- `url`
- `state`
- `is_draft`
- `merge_state_status`
- `head_ref`
- `base_ref`
- `checks[]` with `name`, `status`, `conclusion`

If no PR exists for the branch, Rune returns a structured negative result with `has_pr = false` so orchestrators can decide whether to open one.

## `staged`

`staged` reads `git diff --cached --name-status` and returns structured staged-file details.

### Inputs
- `mode` (optional)
  - `single_commit` — fails if nothing is staged

### Output fields
- `message`
- `count`
- `files[]` with `status` and `path`

Use this before commit creation when an agent needs to validate commit composition without parsing porcelain manually.

## `suggest_branch`

`suggest_branch` normalizes issue/task metadata into a repo-safe branch name.

### Inputs
- `issue` (optional) — numeric issue id to prefix as `issue-<n>`
- `prefix` (optional) — branch namespace, default `agent/rune`
- `name` (optional) — slug source when `args` is omitted
- `args` (optional) — words joined into the slug source

### Output fields
- `suggested`
- `available_locally`

Example:

```json
{
  "operation": "suggest_branch",
  "issue": 773,
  "name": "PR status and branch workflow primitives"
}
```

Returns a branch like `agent/rune/issue-773-pr-status-and-branch-workflow-primitives`.

## Operator notes

- `pr_status`, `staged`, and `suggest_branch` are synthetic operations implemented inside Rune rather than raw git subcommands.
- `pr_status` depends on GitHub CLI availability/auth in the runtime environment.
- Structured JSON output is intentional: orchestration code should consume the JSON contract instead of scraping human-oriented text.
