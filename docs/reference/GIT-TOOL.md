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
- `repo_state` — inspect branch, dirty-tree, protected-branch, and base-sync status in one structured read
- `pr_status` — inspect whether the current or named branch already has a PR, plus merge/check status
- `pr_open` — open a pull request without shelling out to `gh pr create` manually in the agent prompt
- `pr_merge` — merge a pull request with an explicit strategy and delete-branch option
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

## `repo_state`

`repo_state` gives orchestrators a single structured snapshot before mutating git state.

### Inputs
- `base_branch` (optional) — base branch used for remote sync checks, default `main`
- `remote` (optional) — remote used for sync checks, default `origin`
- `protected_branches` (optional) — override protected branch list

### Output fields
- `current_branch`
- `branch_status` — raw `git status --porcelain --branch` branch header
- `dirty`
- `worktree_entries[]`
- `protected_branches[]`
- `on_protected_branch`
- `head_sha`
- `base_sync` with `ok`, `checked`, `base_branch`, `remote`, and sync diagnostics

Use this when an autonomous flow needs to decide whether it is safe to branch, stage, commit, or push without re-implementing multiple git probes in shell.

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

## `pr_open`

`pr_open` wraps `gh pr create` as a structured workflow primitive.

### Inputs
- `title` (required) — PR title
- `body` (optional) — PR body, default empty string
- `base` (optional) — target branch, default `main`
- `head` (optional) — source branch, default current branch

### Output fields
- `message`
- `head`
- `base`
- `url`

## `pr_merge`

`pr_merge` wraps `gh pr merge` so agents can merge by contract instead of shell composition.

### Inputs
- `target` (optional) — PR number/url/branch accepted by `gh pr merge`; defaults to current branch
- `strategy` (optional) — `squash` (default), `merge`, or `rebase`
- `delete_branch` (optional) — default `true`

### Output fields
- `message`
- `target`
- `strategy`
- `delete_branch`
- `details`

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

- `repo_state`, `pr_status`, `pr_open`, `pr_merge`, `staged`, and `suggest_branch` are synthetic operations implemented inside Rune rather than raw git subcommands.
- `pr_status`, `pr_open`, and `pr_merge` depend on GitHub CLI availability/auth in the runtime environment.
- Structured JSON output is intentional: orchestration code should consume the JSON contract instead of scraping human-oriented text.
