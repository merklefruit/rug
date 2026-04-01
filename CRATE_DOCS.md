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
rug reset                                          # clear local addressed state
```

## Configuration

Optional `rug.toml` in the repo root. See [rug.example.toml](./rug.example.toml).

```toml
# Only fix comments from specific authors (default: all)
# review_bots = ["devin-ai[bot]"]

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
  state.rs     .rug/ local state (addressed comment IDs)
  github.rs    gh api graphql calls + response parsing
  verdict.rs   Verdict computation + comment filtering
```

All GitHub API calls go through `gh api graphql` as a subprocess: no HTTP client dependency. 
Auth is inherited from the user's `gh` session or `GITHUB_TOKEN` env var.
