---
name: review-until-green
description: Fix PR review comments and CI failures in a loop until green. Requires `rug` CLI.
---

# review-until-green

Fix a PR until it's green: every review comment addressed, CI passing, and — when Copilot or CodeRabbit is a reviewer — it has reviewed the latest code. Use the `rug` CLI for ALL GitHub state and actions. Never call `gh` directly.

**Requires:** `rug` on PATH. Install: `cargo install --git https://github.com/merklefruit/rug`
**Setup:** Ensure `.rug/` is in `.gitignore` (stores local delta-tracking state).

## Commands

PR ref is an optional positional arg BEFORE the subcommand. Omit = current branch.

| Command                              | Purpose                                            |
| ------------------------------------ | -------------------------------------------------- |
| `rug [pr] status`                    | JSON: verdict, new_comments, ci, reviews, summary  |
| `rug [pr] checks`                    | JSON: ci status only (lightweight poll)            |
| `rug [pr] watch`                     | Block until checks settle, then print full status  |
| `rug [pr] mark-addressed <ids...>`   | Mark comment IDs as fixed locally                  |
| `rug [pr] request-copilot-review`    | Re-request a GitHub Copilot review                 |
| `rug [pr] request-coderabbit-review` | Ask CodeRabbit to review the latest push           |
| `rug [pr] reset`                     | Clear local addressed state                        |

`status`/`watch` also include:

- `reviews`: `[{ author, state, stale }]` — each reviewer's latest overall review.
  `state` is `approved`/`changes_requested`/`commented`/`dismissed`/`pending`.
  `stale: true` means the review predates the current HEAD (the reviewer hasn't
  seen the latest push). Omitted when there are no reviews.
- `requested_reviewers`: logins whose review is currently requested but not yet
  submitted (pending). Omitted when empty.
- `coderabbit`: `{ state, detail }` — CodeRabbit's progress on the current HEAD,
  read from the `CodeRabbit` status it posts on each commit (falling back to
  review freshness when there is none). `state` is
  `reviewed`/`reviewing`/`paused`/`skipped`/`rate_limited`/`failed`/`not_reviewed`;
  `detail` is CodeRabbit's own status text. Omitted unless CodeRabbit has
  reviewed this PR.

## Loop (max 5 iterations)

```
START:
  run `rug [pr] watch`        # blocks until CI + review bots settle
  parse JSON output (same schema as `rug status`)
  `watch` timed out → run `rug [pr] status` once: FIX_COMMENTS if new_comments
    is non-empty, else stop and report the checks still pending

  verdict=approved  → REREVIEW_GATE
  verdict=merged    → DONE (tell user, stop)
  verdict=closed    → DONE (tell user, stop)
  verdict=changes_requested → FIX_COMMENTS
  verdict=ci_failing → FIX_CI
  verdict=pending   → FIX_COMMENTS if new_comments is non-empty, else START

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
  fix them, then git add + commit + push
  a failing `CodeRabbit` check is CodeRabbit's own review erroring, not CI:
    don't change code for it; run `rug [pr] request-coderabbit-review`
  nothing pushed and no CodeRabbit review requested (requested == false)
    → stop, report
  → START

REREVIEW_GATE:                # all comments addressed + CI green
  Check each AI reviewer below. Any WAIT → START (the next `watch` waits for
  the fresh review). Otherwise → DONE (summarize, stop).

  Copilot: its latest review in `reviews`, the entry whose `author`
  (case-insensitive) is `copilot-pull-request-reviewer` or `copilot`.
    no Copilot review           → ok (Copilot is not a reviewer)
    state == "approved"         → ok
    stale == false              → ok (Copilot already reviewed this HEAD*)
    stale, login in `requested_reviewers` → WAIT (re-review already pending)
    stale otherwise             → run `rug [pr] request-copilot-review`; WAIT

  CodeRabbit: the `coderabbit` object.
    omitted                     → ok (CodeRabbit is not a reviewer)
    state == "reviewed"         → ok
    state == "reviewing"        → WAIT
    state == "rate_limited"     → ok; report it (a request would be rate limited too)
    any other state             → run `rug [pr] request-coderabbit-review`
                                  requested == true  → WAIT
                                  requested == false → ok; report it**

  * Copilot only ever leaves "commented" reviews (it never "approves"), so the
    stop condition is "Copilot has reviewed the current HEAD", NOT "approved".
    Gating on stale prevents an infinite re-request loop.
  ** rug asks CodeRabbit at most once per HEAD, so false means it was already
    asked about this HEAD and still hasn't reviewed it. Don't judge CodeRabbit
    by `reviews[].stale`: it can finish a commit without submitting a review.
```

## Rules

1. Only read files named in comments. Never read entire PR diff.
2. Only fix what's requested. No refactoring.
3. All GitHub state and actions via `rug` (including `request-copilot-review` and `request-coderabbit-review`). No `gh api`, no `gh pr`, and never post `@coderabbitai` commands yourself.
4. Always `mark-addressed` after push so delta tracking works.
5. If stuck on a comment, stop loop, report to user.
6. One commit per fix cycle.
7. Re-request reviews only as REREVIEW_GATE (or FIX_CI, for a failing `CodeRabbit` check) says: Copilot only when its latest review is `stale` and Copilot is not already in `requested_reviewers`, CodeRabbit only when `coderabbit.state` says it hasn't reviewed HEAD. Never re-request in a tight loop, and don't re-request other reviewers.

## Summary (on exit)

Report: final verdict, loop count, comments addressed (file:line + description), any Copilot or CodeRabbit re-reviews requested, unresolved items (including a CodeRabbit that was rate limited or didn't review HEAD).
