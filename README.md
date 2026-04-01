# `rug` - Review Until Green

Agent skill and CLI for **token-efficient** GitHub PR review loops: your agent gets compact, delta-tracked state from `rug` instead of re-fetching the full PR picture every time.

## The flow

Use this after AI reviewers have left a pile of small comments—not instead of human review at the end.

```mermaid
%%{init: {'flowchart': {'nodeSpacing': 28, 'rankSpacing': 32}}}%%
flowchart TB
    A([You commit your work]) --> B[AI tools review it (Claude, Greptile, Devin, CodeRabbit, Cursor, …)]
    B --> C[Your PR littered with constant rounds of comments—trivial to fix on frontier models]
    C --> D([You summon /review-until-green])
    D --> E{Every AI comment addressed?}
    E -->|no| L[Fix, push, poll CI & reviews]
    L --> E
    E -->|yes| F([Manual pass on what changed, then human team review])
```

## Why tokens balloon

When you ask an agent to “fix this PR until it’s green,” **each iteration** usually re-pulls review threads, CI, and “what’s left” from the GitHub API and re-parses all of it. That work compounds every loop.

## What you get

**`rug` CLI** — Talks to GitHub (via `gh`), returns **compact JSON**, and keeps **local** “already addressed” state so each call can return **only new** comments. Token cost stays flatter as the PR grows.

**`/review-until-green` skill** — Drives the loop: call `rug` for state, apply fixes, push, mark addressed, poll CI/reviews, repeat until the verdict is `approved` (best-effort). It does **not** call GitHub’s API itself—only `rug`.

## Setup (for your agent)

```md
Install the skill at https://github.com/merklefruit/rug/blob/main/INSTALL.md and explain how to use it
```

## Setup (manual)

You need both the CLI and the skill:

```sh
# Install CLI (requires gh CLI authenticated)
cargo install --git https://github.com/merklefruit/rug

# Install skill
npx skills add merklefruit/rug
```

See [INSTALL.md](./INSTALL.md) for manual install and config options.

## Usage

```
/review-until-green
/review-until-green https://github.com/owner/repo/pull/123
```

## CLI commands

The skill uses `rug` for all GitHub state:

```
rug [pr] status          → verdict + new comments + CI (delta only)
rug [pr] checks          → CI status (lightweight poll)
rug [pr] mark-addressed  → track fixed comment IDs locally
rug [pr] reset           → clear local state
```

Omit `[pr]` to use the current branch. Accepts URLs or `owner/repo#123`.

## Config

Optional `rug.toml` in repo root — see [rug.example.toml](./rug.example.toml).

Works with any bot or person that posts PR review comments (same idea as the tools listed in the diagram).

## Docs

- [INSTALL.md](./INSTALL.md) — detailed setup
- [CRATE_DOCS.md](./CRATE_DOCS.md) — CLI internals, architecture

## License

MIT
