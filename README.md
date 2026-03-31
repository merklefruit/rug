# Review Until Green

Automate the PR review-fix loop. A CLI (`rug`) feeds compact, delta-tracked review state to a Claude Code skill that fixes issues, pushes, and repeats until the PR is green.

## The problem

When you ask a coding agent to "fix this PR until it's approved," it spends most of its tokens re-discovering PR state from the GitHub API on every iteration. Review comments, CI status, what's already been addressed — the agent fetches and parses all of it from scratch each loop.

## The solution

**`rug` CLI** — fetches PR review comments and CI status from GitHub, returns compact JSON, and tracks what's been addressed locally. Each call returns only the delta — new comments the agent hasn't seen yet. Token usage stays flat across loops instead of growing with PR history.

**`/review-until-green` skill** — orchestrates the fix loop. Calls `rug` for all GitHub state, fixes the issues, pushes, marks comments as addressed, polls for CI and new reviews, and repeats until the verdict is `approved` or it hits the max loop cap.

## Setup

```sh
# Install CLI (requires gh CLI authenticated)
cargo install --git https://github.com/merklefruit/review-until-green

# Install skill
npx skills add merklefruit/review-until-green
```

See [INSTALL.md](./INSTALL.md) for manual install and config options.

## Usage

```
/review-until-green
/review-until-green https://github.com/owner/repo/pull/123
```

The skill loops automatically: fetch status, fix comments/CI, push, poll, repeat.

## How it works

The skill calls `rug` for all GitHub state — never the API directly.

```
rug [pr] status          → verdict + new comments + CI (delta only)
rug [pr] checks          → CI status (lightweight poll)
rug [pr] mark-addressed  → track fixed comment IDs locally
rug [pr] reset           → clear local state
```

Omit `[pr]` to use the current branch. Accepts URLs or `owner/repo#123`.

## Config

Optional `rug.toml` in repo root — see [rug.toml.example](./rug.toml.example).

Works with any bot that posts PR review comments: Devin, Cursor, CodeRabbit, human reviewers, etc.

## Docs

- [INSTALL.md](./INSTALL.md) — detailed setup
- [CRATE_DOCS.md](./CRATE_DOCS.md) — CLI internals, architecture

## License

MIT
