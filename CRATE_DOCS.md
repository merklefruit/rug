# rug — Crate Documentation

Compact PR review state for coding agents.

`rug` is a Rust CLI that fetches PR review comments and CI status from GitHub via `gh api graphql`, returns compact JSON, and tracks which comments have been addressed locally in `.rug/` for delta-only output.

## Commands

PR is an optional positional arg before the subcommand. Omit it to use the current branch's PR.

```text
rug status                                         # current branch
rug https://github.com/owner/repo/pull/123 status  # explicit URL
rug owner/repo#123 checks                          # short ref
rug watch                                          # block until checks settle, print status
rug watch --timeout 300 --settle 30                # custom timeout and settle window
rug mark-addressed 1001 1002                       # mark comment IDs as addressed
rug request-copilot-review                         # request a GitHub Copilot review
rug request-coderabbit-review                      # ask CodeRabbit to review the latest push
rug request-coderabbit-review --full               # ...or to review the whole PR again
rug reset                                          # clear local addressed state
```

`status` and `watch` also report `reviews` — each reviewer's latest overall
review as `{ author, state, stale }`, where `stale` is true when the review
predates the current HEAD — plus `requested_reviewers` (pending review requests).
`request-copilot-review` re-requests a Copilot review via `gh pr edit
--add-reviewer @copilot` (the REST `requested_reviewers` endpoint does not accept
Copilot); it needs a `gh` version that supports `@copilot` as a reviewer.

When CodeRabbit has reviewed the PR, or rug has asked it to (which is how the
skill adds it as a reviewer), `status` and `watch` also report
`coderabbit`: its progress on HEAD as `{ state, detail }` (`reviewing`,
`reviewed`, `paused`, `skipped`, `rate_limited`, `failed`, or `not_reviewed`).
It is read from the `CodeRabbit` commit status CodeRabbit sets on each commit,
because CodeRabbit can finish a commit without submitting a new review; review
freshness is only the fallback when there is no status. It is omitted when
`review_bots` leaves CodeRabbit out. While CodeRabbit is reviewing, `watch`
returns as soon as new comments arrive, because CodeRabbit posts its review
minutes before it marks the review completed.

CodeRabbit takes commands from PR comments rather than review requests, so
`request-coderabbit-review` posts `@coderabbitai review` (`@coderabbitai full
review` with `--full`), then waits up to a minute for CodeRabbit to start
reviewing. It posts at most once per HEAD, tracked in `.rug/`; pass `--force` to
ask again. CodeRabbit ignores commands from bot accounts, so `gh` must be
authenticated as a user, not with the Actions `GITHUB_TOKEN`.

## Configuration

Optional `rug.toml` in the repo root. See [rug.example.toml](./rug.example.toml).

```toml
# Only fix comments from specific authors (default: all).
# Logins match case-insensitively, with or without the `[bot]` suffix.
# review_bots = ["coderabbitai[bot]"]

# Seconds to wait after checks settle (default: 60)
settle_window = 60

# Max fix loops (default: 5)
max_loops = 5
```

Add `.rug/` to your `.gitignore`.

## Architecture

```text
src/
  main.rs      CLI entry, clap setup, command dispatch
  types.rs     GraphQL response types, domain model, JSON output types
  config.rs    rug.toml parsing
  pr.rs        PR URL parsing + branch-to-PR resolution via gh
  state.rs     .rug/ local state (addressed comment IDs, CodeRabbit requests)
  github.rs    gh api graphql calls + response parsing
  verdict.rs   Verdict computation + comment filtering
```

All GitHub reads go through `gh api graphql` as a subprocess: no HTTP client dependency.
The two write actions shell out to `gh pr`: `request-copilot-review` runs `gh pr edit --add-reviewer @copilot`,
and `request-coderabbit-review` runs `gh pr comment`.
Auth is inherited from the user's `gh` session or `GITHUB_TOKEN` env var.
