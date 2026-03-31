---
name: review-until-green
description: Fix PR review comments and CI failures in a loop until green. Requires `rug` CLI.
---

# review-until-green

Fix PR until verdict=approved. Use `rug` CLI for ALL GitHub state. Never call `gh api` directly.

**Requires:** `rug` on PATH. Install: `cargo install --git https://github.com/merklefruit/review-until-green`
**Setup:** Ensure `.rug/` is in `.gitignore` (stores local delta-tracking state).

## Commands

PR ref is an optional positional arg BEFORE the subcommand. Omit = current branch.

| Command                            | Purpose                                  |
| ---------------------------------- | ---------------------------------------- |
| `rug [pr] status`                  | JSON: verdict, new_comments, ci, summary |
| `rug [pr] checks`                  | JSON: ci status only (lightweight poll)  |
| `rug [pr] mark-addressed <ids...>` | Mark comment IDs as fixed locally        |
| `rug [pr] reset`                   | Clear local addressed state              |

## Loop (max 5 iterations)

```
START:
  run `rug [pr] status`

  verdict=approved  → DONE (summarize, stop)
  verdict=merged    → DONE (tell user, stop)
  verdict=closed    → DONE (tell user, stop)
  verdict=pending   → POLL
  verdict=changes_requested → FIX_COMMENTS
  verdict=ci_failing → FIX_CI

FIX_COMMENTS:
  for each item in new_comments:
    read file at `path` around `line`
    understand issue from `body`, `diff_hunk`, `replies`
    fix it
  git add + commit + push
  run `rug [pr] mark-addressed <id1> <id2> ...`
  → POLL

FIX_CI:
  inspect failing checks from ci.checks (use URLs)
  fix issues
  git add + commit + push
  → POLL

POLL:
  sleep 30s
  run `rug [pr] checks`
  if all_settled=false → sleep 30s, repeat POLL
  if all_settled=true  → sleep 60s (settle window), → START
```

## Rules

1. Only read files named in comments. Never read entire PR diff.
2. Only fix what's requested. No refactoring.
3. All GitHub state via `rug`. No `gh api`, no `gh pr`.
4. Always `mark-addressed` after push so delta tracking works.
5. If stuck on a comment, stop loop, report to user.
6. One commit per fix cycle.

## Summary (on exit)

Report: final verdict, loop count, comments addressed (file:line + description), unresolved items.
