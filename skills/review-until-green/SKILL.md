---
name: review-until-green
description: Fix PR review comments and CI failures in a loop until green. Requires `rug` CLI.
---

# review-until-green

Fix a PR until it's green: every review comment addressed, CI passing, and — when Copilot is a reviewer — Copilot has re-reviewed the latest code. Use the `rug` CLI for ALL GitHub state and actions. Never call `gh` directly.

**Requires:** `rug` on PATH. Install: `cargo install --git https://github.com/merklefruit/rug`
**Setup:** Ensure `.rug/` is in `.gitignore` (stores local delta-tracking state).

## Commands

PR ref is an optional positional arg BEFORE the subcommand. Omit = current branch.

| Command                            | Purpose                                            |
| ---------------------------------- | -------------------------------------------------- |
| `rug [pr] status`                  | JSON: verdict, new_comments, ci, reviews, summary  |
| `rug [pr] checks`                  | JSON: ci status only (lightweight poll)            |
| `rug [pr] watch`                   | Block until checks settle, then print full status  |
| `rug [pr] mark-addressed <ids...>` | Mark comment IDs as fixed locally                  |
| `rug [pr] request-copilot-review`  | Re-request a GitHub Copilot review                 |
| `rug [pr] reset`                   | Clear local addressed state                        |

`status`/`watch` also include:

- `reviews`: `[{ author, state, stale }]` — each reviewer's latest overall review.
  `state` is `approved`/`changes_requested`/`commented`/`dismissed`/`pending`.
  `stale: true` means the review predates the current HEAD (the reviewer hasn't
  seen the latest push). Omitted when there are no reviews.
- `requested_reviewers`: logins whose review is currently requested but not yet
  submitted (pending). Omitted when empty.

## Loop (max 5 iterations)

```
START:
  run `rug [pr] watch`        # blocks until CI + review bots settle
  parse JSON output (same schema as `rug status`)

  verdict=approved  → COPILOT_GATE
  verdict=merged    → DONE (tell user, stop)
  verdict=closed    → DONE (tell user, stop)
  verdict=changes_requested → FIX_COMMENTS
  verdict=ci_failing → FIX_CI

FIX_COMMENTS:
  for each item in new_comments:
    read file at `path` around `line`
    understand issue from `body`, `diff_hunk`, `replies`
    fix it
  git add + commit + push
  run `rug [pr] mark-addressed <id1> <id2> ...`
  → START

FIX_CI:
  inspect failing checks from ci.checks (use URLs)
  fix issues
  git add + commit + push
  → START

COPILOT_GATE:                 # all comments addressed + CI green
  In `reviews`, find Copilot's latest review: an entry whose `author`
  (case-insensitive) is `copilot-pull-request-reviewer` or `copilot`.

  no Copilot review            → DONE (Copilot was not a reviewer; summarize, stop)
  its state == "approved"      → DONE
  its stale == false           → DONE (Copilot already reviewed this HEAD*)
  else (stale, not approved):  # Copilot reviewed an older commit — re-ask it
    if Copilot's login is NOT in `requested_reviewers`:   # none already pending
      run `rug [pr] request-copilot-review`
    → START                    # next `watch` waits for Copilot's fresh review

  * Copilot only ever leaves "commented" reviews (it never "approves"), so the
    stop condition is "Copilot has reviewed the current HEAD", NOT "approved".
    Gating on stale prevents an infinite re-request loop.
```

## Rules

1. Only read files named in comments. Never read entire PR diff.
2. Only fix what's requested. No refactoring.
3. All GitHub state and actions via `rug` (including `request-copilot-review`). No `gh api`, no `gh pr`.
4. Always `mark-addressed` after push so delta tracking works.
5. If stuck on a comment, stop loop, report to user.
6. One commit per fix cycle.
7. Re-request Copilot only when its latest review is `stale` and Copilot is not already in `requested_reviewers`. Never re-request in a tight loop, and don't re-request other reviewers.

## Summary (on exit)

Report: final verdict, loop count, comments addressed (file:line + description), any Copilot re-reviews requested, unresolved items.
