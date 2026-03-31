---
name: review-until-green
description: Fix PR review comments and CI failures in a loop until the PR is green. Requires `rug` CLI on PATH.
---

# Review Until Green

You are fixing a PR until all review comments are addressed and CI is green. You will use the `rug` CLI for all GitHub state — never call the GitHub API directly.

## Input

The user may provide a PR URL as an argument. If not, you are working on the current branch's PR.

## Setup

1. Determine the PR reference:
   - If the user provided a URL/ref, use it: `rug status <pr-url>`
   - If not, use current branch: `rug status`

2. Read the config by running `rug status` and noting the output.

## Loop

Repeat the following steps. Stop after 5 iterations (or the max_loops from rug.toml) or when the verdict is "approved".

### Step 1: Get Status

Run: `rug status [pr-url]`

Parse the JSON output. Check the `verdict` field:
- `"approved"` → Done! Summarize and stop.
- `"merged"` → PR is already merged. Tell the user and stop.
- `"closed"` → PR is closed. Tell the user and stop.
- `"pending"` → CI is still running. Go to the Polling step.
- `"changes_requested"` → There are review comments to fix. Go to Step 2.
- `"ci_failing"` → CI is failing but no new review comments. Go to Step 3.

### Step 2: Fix Review Comments

For each comment in `new_comments`:
1. Read the file at `path` (focus on the area around `line`)
2. Understand the issue from `body`, `diff_hunk`, and any `replies`
3. Make the fix

After fixing all comments:
1. `git add` the changed files
2. `git commit -m "fix: address review comments"` (include specifics in the message)
3. `git push`
4. Run: `rug mark-addressed [pr-url] <id1> <id2> ...` with all comment IDs you addressed
5. Go to the Polling step.

### Step 3: Fix CI Failures

1. Look at the failing checks in the `ci.checks` array
2. Visit the check URLs or inspect the code to understand failures
3. Fix the issues
4. `git add`, `git commit -m "fix: resolve CI failures"`, `git push`
5. Go to the Polling step.

### Polling Step

Wait for CI and review bots to process the new push:

1. Wait 30 seconds
2. Run: `rug checks [pr-url]`
3. If `all_settled` is `false`, wait another 30 seconds and repeat
4. Once `all_settled` is `true`, wait 60 more seconds (settle window for review bots)
5. Go back to Step 1 for the next loop iteration

## Important Rules

- **Only read files mentioned in comments.** Do not read the entire PR diff.
- **Only fix what's requested.** Do not refactor or improve surrounding code.
- **Use `rug` for all GitHub state.** Do not run `gh api` or other GitHub CLI commands.
- **Mark comments as addressed** after pushing fixes so you don't see them again.
- **Stop if stuck.** If you cannot figure out how to fix a comment, report it to the user and stop the loop.
- **Keep commits small and focused.** One commit per fix cycle is ideal.

## Output

When done (or when max loops reached), provide a summary:
- Final verdict
- Number of loops taken
- Which comments were addressed (file + line + brief description)
- Any remaining unresolved items
