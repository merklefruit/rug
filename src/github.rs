//! GitHub API client using `gh api graphql`.
//!
//! All GitHub communication goes through the `gh` CLI as a subprocess.
//! This avoids needing an HTTP client dependency and inherits the user's
//! existing `gh` authentication.

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::types::*;

const FULL_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number
      state
      headRefOid
      reviewThreads(first: 100) {
        nodes {
          isResolved
          comments(first: 50) {
            nodes {
              databaseId
              author { login }
              body
              path
              line
              diffHunk
              url
              createdAt
            }
          }
        }
      }
      latestReviews(first: 100) {
        nodes {
          author { login }
          state
          commit { oid }
        }
      }
      reviewRequests(first: 100) {
        nodes {
          requestedReviewer {
            __typename
            ... on Bot { login }
            ... on User { login }
            ... on Mannequin { login }
          }
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            pushedDate
            status {
              context(name: "CodeRabbit") {
                state
                description
                targetUrl
              }
            }
            statusCheckRollup {
              state
              contexts(first: 100) {
                nodes {
                  __typename
                  ... on CheckRun {
                    name
                    status
                    conclusion
                    detailsUrl
                    title
                  }
                  ... on StatusContext {
                    context
                    state
                    targetUrl
                    description
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#;

const CHECKS_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number
      state
      headRefOid
      commits(last: 1) {
        nodes {
          commit {
            pushedDate
            status {
              context(name: "CodeRabbit") {
                state
                description
                targetUrl
              }
            }
            statusCheckRollup {
              state
              contexts(first: 100) {
                nodes {
                  __typename
                  ... on CheckRun {
                    name
                    status
                    conclusion
                    detailsUrl
                  }
                  ... on StatusContext {
                    context
                    state
                    targetUrl
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#;

fn call_gh_graphql(pr: &PrRef, query: &str) -> Result<String> {
    let output = Command::new("gh")
        .args([
            "api",
            "graphql",
            "-f",
            &format!("query={query}"),
            "-f",
            &format!("owner={}", pr.owner),
            "-f",
            &format!("name={}", pr.repo),
            "-F",
            &format!("number={}", pr.number),
        ])
        .output()
        .context("Failed to run `gh api graphql`. Is gh CLI installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("GitHub API error: {stderr}");
    }

    String::from_utf8(output.stdout).context("Invalid UTF-8 from GitHub API")
}

/// Parse and unwrap a GraphQL response to the inner PullRequest node.
fn unwrap_pr_gql(json: &str) -> Result<PullRequestGql> {
    let response: GraphQLResponse =
        serde_json::from_str(json).context("Failed to parse GitHub GraphQL response")?;

    if let Some(errors) = response.errors {
        let msgs: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        bail!("GitHub API errors: {}", msgs.join("; "));
    }

    response
        .data
        .context("No data in GitHub response")?
        .repository
        .pull_request
        .context("Pull request not found")
}

fn author_login(author: &Option<AuthorGql>) -> String {
    author.as_ref().map(|a| a.login.clone()).unwrap_or_else(|| "unknown".to_string())
}

/// Parse GraphQL response JSON into domain types.
pub fn parse_pr_response(json: &str) -> Result<PrData> {
    let pr_gql = unwrap_pr_gql(json)?;

    let threads = parse_threads(&pr_gql);
    let reviews = parse_reviews(&pr_gql);
    let requested_reviewers = parse_requested_reviewers(&pr_gql);
    let (checks, pushed_at) = parse_checks(&pr_gql);

    Ok(PrData {
        number: pr_gql.number,
        repo: String::new(), // Filled in by caller
        state: PrState::from_gql(&pr_gql.state),
        head_sha: pr_gql.head_ref_oid,
        threads,
        checks,
        reviews,
        requested_reviewers,
        pushed_at,
    })
}

fn parse_reviews(pr: &PullRequestGql) -> Vec<Review> {
    let Some(reviews) = &pr.latest_reviews else {
        return vec![];
    };

    reviews
        .nodes
        .iter()
        .map(|r| Review {
            author: author_login(&r.author),
            state: ReviewState::from_gql(&r.state),
            commit_oid: r.commit.as_ref().map(|c| c.oid.clone()),
        })
        .collect()
}

fn parse_requested_reviewers(pr: &PullRequestGql) -> Vec<String> {
    let Some(requests) = &pr.review_requests else {
        return vec![];
    };

    requests
        .nodes
        .iter()
        .filter_map(|n| n.requested_reviewer.as_ref().and_then(|r| r.login.clone()))
        .collect()
}

fn parse_threads(pr: &PullRequestGql) -> Vec<Thread> {
    let Some(threads) = &pr.review_threads else {
        return vec![];
    };

    threads
        .nodes
        .iter()
        .filter_map(|t| {
            let comments = &t.comments.nodes;
            let first = comments.first()?;
            let id = first.database_id?; // skip comments with no database ID
            let first_comment = Comment {
                id,
                author: author_login(&first.author),
                path: first.path.clone(),
                line: first.line,
                body: first.body.clone(),
                diff_hunk: first.diff_hunk.clone(),
                url: first.url.clone(),
            };
            let replies: Vec<Reply> = comments
                .iter()
                .skip(1)
                .map(|c| Reply { author: author_login(&c.author), body: c.body.clone() })
                .collect();

            Some(Thread { is_resolved: t.is_resolved, first_comment, replies })
        })
        .collect()
}

fn parse_checks(pr: &PullRequestGql) -> (Vec<Check>, Option<String>) {
    let Some(commit_node) = pr.commits.nodes.first() else {
        return (vec![], None);
    };

    let pushed_at = commit_node.commit.pushed_date.clone();

    let Some(rollup) = &commit_node.commit.status_check_rollup else {
        return (vec![], pushed_at);
    };

    let mut checks: Vec<Check> = rollup
        .contexts
        .nodes
        .iter()
        .map(|ctx| match ctx {
            CheckContextGql::CheckRun { name, status, conclusion, details_url, title } => Check {
                name: name.clone(),
                status: check_run_status(status, conclusion.as_deref()),
                url: details_url.clone(),
                detail: title.clone(),
            },
            CheckContextGql::StatusContext { context, state, target_url, description } => Check {
                name: context.clone(),
                status: status_context_state(state),
                url: target_url.clone(),
                detail: description.clone(),
            },
        })
        .collect();

    // The rollup's page of contexts can miss CodeRabbit's status on repos with many checks
    let coderabbit = commit_node.commit.status.as_ref().and_then(|s| s.context.as_ref());
    if let Some(ctx) = coderabbit.filter(|_| !checks.iter().any(|c| c.name == CODERABBIT_STATUS)) {
        checks.push(Check {
            name: CODERABBIT_STATUS.to_string(),
            status: status_context_state(&ctx.state),
            url: ctx.target_url.clone(),
            detail: ctx.description.clone(),
        });
    }

    (checks, pushed_at)
}

fn check_run_status(status: &str, conclusion: Option<&str>) -> CheckStatus {
    match status {
        "COMPLETED" => match conclusion {
            Some("SUCCESS") => CheckStatus::Success,
            Some("NEUTRAL") => CheckStatus::Neutral,
            Some("SKIPPED") => CheckStatus::Skipped,
            _ => CheckStatus::Failure,
        },
        "IN_PROGRESS" => CheckStatus::InProgress,
        _ => CheckStatus::Queued,
    }
}

fn status_context_state(state: &str) -> CheckStatus {
    match state {
        "SUCCESS" => CheckStatus::Success,
        "PENDING" | "EXPECTED" => CheckStatus::InProgress,
        _ => CheckStatus::Failure,
    }
}

/// Parse a checks-only GraphQL response.
pub fn parse_checks_response(json: &str) -> Result<(Vec<Check>, Option<String>)> {
    let pr_gql = unwrap_pr_gql(json)?;
    Ok(parse_checks(&pr_gql))
}

/// Fetch full PR data from GitHub.
pub fn fetch_pr_data(pr: &PrRef) -> Result<PrData> {
    let json = call_gh_graphql(pr, FULL_QUERY)?;
    let mut data = parse_pr_response(&json)?;
    data.repo = format!("{}/{}", pr.owner, pr.repo);
    Ok(data)
}

/// Fetch only check/CI data from GitHub.
pub fn fetch_checks(pr: &PrRef) -> Result<(Vec<Check>, Option<String>)> {
    let json = call_gh_graphql(pr, CHECKS_QUERY)?;
    parse_checks_response(&json)
}

/// Fetch the SHA of the PR's current HEAD commit.
pub fn fetch_head_sha(pr: &PrRef) -> Result<String> {
    let json = call_gh_graphql(pr, CHECKS_QUERY)?;
    Ok(unwrap_pr_gql(&json)?.head_ref_oid)
}

/// Re-request a code review from GitHub Copilot on the given PR.
///
/// Uses `gh pr edit --add-reviewer @copilot`, the documented mechanism for
/// requesting a Copilot review. The REST `requested_reviewers` endpoint does
/// not accept Copilot, so this cannot go through `gh api`. Requires a `gh`
/// version that supports `@copilot` as a reviewer (March 2026 or later).
pub fn request_copilot_review(pr: &PrRef) -> Result<()> {
    let output = Command::new("gh")
        .args([
            "pr",
            "edit",
            &pr.number.to_string(),
            "--repo",
            &format!("{}/{}", pr.owner, pr.repo),
            "--add-reviewer",
            "@copilot",
        ])
        .output()
        .context("Failed to run `gh pr edit`. Is gh CLI installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Failed to request Copilot review: {stderr}\n\
             Ensure Copilot code review is enabled for the repo and that `gh` is recent \
             enough to support `--add-reviewer @copilot` (gh 2.x, March 2026+)."
        );
    }

    Ok(())
}

/// Ask CodeRabbit to review the given PR.
///
/// CodeRabbit takes commands from PR comments and can't be added as a requested reviewer, so
/// this posts `@coderabbitai review` (incremental: the commits since its last review) or, with
/// `full`, `@coderabbitai full review` (the whole PR from scratch). Both work while CodeRabbit's
/// automatic reviews are paused.
pub fn request_coderabbit_review(pr: &PrRef, full: bool) -> Result<()> {
    let body = if full { "@coderabbitai full review" } else { "@coderabbitai review" };
    let output = Command::new("gh")
        .args([
            "pr",
            "comment",
            &pr.number.to_string(),
            "--repo",
            &format!("{}/{}", pr.owner, pr.repo),
            "--body",
            body,
        ])
        .output()
        .context("Failed to run `gh pr comment`. Is gh CLI installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to request CodeRabbit review: {stderr}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_full_response() -> &'static str {
        r#"{
  "data": {
    "repository": {
      "pullRequest": {
        "number": 42,
        "state": "OPEN",
        "headRefOid": "abc123def",
        "reviewThreads": {
          "nodes": [
            {
              "isResolved": false,
              "comments": {
                "nodes": [
                  {
                    "databaseId": 1001,
                    "author": { "login": "devin-ai[bot]" },
                    "body": "Missing error handling here",
                    "path": "src/main.rs",
                    "line": 42,
                    "diffHunk": "@@ -40,3 +40,5 @@ fn process()",
                    "url": "https://github.com/octocat/repo/pull/42#discussion_r1001",
                    "createdAt": "2026-03-31T10:00:00Z"
                  },
                  {
                    "databaseId": 1002,
                    "author": { "login": "reviewer" },
                    "body": "Agreed, also check line 50",
                    "path": "src/main.rs",
                    "line": 42,
                    "diffHunk": "@@ -40,3 +40,5 @@ fn process()",
                    "url": "https://github.com/octocat/repo/pull/42#discussion_r1002",
                    "createdAt": "2026-03-31T10:05:00Z"
                  }
                ]
              }
            },
            {
              "isResolved": true,
              "comments": {
                "nodes": [
                  {
                    "databaseId": 2001,
                    "author": { "login": "devin-ai[bot]" },
                    "body": "This was already fixed",
                    "path": "src/lib.rs",
                    "line": 10,
                    "diffHunk": "@@ -8,3 +8,5 @@",
                    "url": "https://github.com/octocat/repo/pull/42#discussion_r2001",
                    "createdAt": "2026-03-31T09:00:00Z"
                  }
                ]
              }
            }
          ]
        },
        "latestReviews": {
          "nodes": [
            {
              "author": { "login": "copilot-pull-request-reviewer" },
              "state": "COMMENTED",
              "commit": { "oid": "oldsha111" }
            },
            {
              "author": { "login": "human-reviewer" },
              "state": "APPROVED",
              "commit": { "oid": "abc123def" }
            }
          ]
        },
        "reviewRequests": {
          "nodes": [
            {
              "requestedReviewer": {
                "__typename": "Bot",
                "login": "copilot-pull-request-reviewer"
              }
            },
            {
              "requestedReviewer": {
                "__typename": "User",
                "login": "alice"
              }
            },
            {
              "requestedReviewer": {
                "__typename": "Team"
              }
            }
          ]
        },
        "commits": {
          "nodes": [
            {
              "commit": {
                "pushedDate": "2026-03-31T09:30:00Z",
                "statusCheckRollup": {
                  "state": "FAILURE",
                  "contexts": {
                    "nodes": [
                      {
                        "__typename": "CheckRun",
                        "name": "tests",
                        "status": "COMPLETED",
                        "conclusion": "FAILURE",
                        "detailsUrl": "https://github.com/octocat/repo/actions/runs/1",
                        "title": "2 tests failed"
                      },
                      {
                        "__typename": "CheckRun",
                        "name": "lint",
                        "status": "COMPLETED",
                        "conclusion": "SUCCESS",
                        "detailsUrl": "https://github.com/octocat/repo/actions/runs/2"
                      },
                      {
                        "__typename": "StatusContext",
                        "context": "deploy/preview",
                        "state": "PENDING",
                        "targetUrl": "https://preview.example.com",
                        "description": "Deploying preview"
                      }
                    ]
                  }
                }
              }
            }
          ]
        }
      }
    }
  }
}"#
    }

    #[test]
    fn test_parse_pr_response() {
        let data = parse_pr_response(fixture_full_response()).unwrap();
        assert_eq!(data.number, 42);
        assert_eq!(data.state, PrState::Open);
        assert_eq!(data.head_sha, "abc123def");
    }

    #[test]
    fn test_parse_threads() {
        let data = parse_pr_response(fixture_full_response()).unwrap();
        assert_eq!(data.threads.len(), 2);

        let unresolved: Vec<_> = data.threads.iter().filter(|t| !t.is_resolved).collect();
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].first_comment.id, 1001);
        assert_eq!(unresolved[0].first_comment.author, "devin-ai[bot]");
        assert_eq!(unresolved[0].first_comment.body, "Missing error handling here");
        assert_eq!(unresolved[0].first_comment.path.as_deref(), Some("src/main.rs"));
        assert_eq!(unresolved[0].first_comment.line, Some(42));
        assert_eq!(unresolved[0].replies.len(), 1);
        assert_eq!(unresolved[0].replies[0].author, "reviewer");
    }

    #[test]
    fn test_parse_checks() {
        let data = parse_pr_response(fixture_full_response()).unwrap();
        assert_eq!(data.checks.len(), 3);

        assert_eq!(data.checks[0].name, "tests");
        assert_eq!(data.checks[0].status, CheckStatus::Failure);

        assert_eq!(data.checks[1].name, "lint");
        assert_eq!(data.checks[1].status, CheckStatus::Success);

        assert_eq!(data.checks[2].name, "deploy/preview");
        assert_eq!(data.checks[2].status, CheckStatus::InProgress);
    }

    /// A checks-only response whose rollup page has `contexts` and whose CodeRabbit status,
    /// looked up by name, is `coderabbit`.
    fn checks_response(contexts: &str, coderabbit: &str) -> String {
        format!(
            r#"{{"data": {{"repository": {{"pullRequest": {{
                "number": 1, "state": "OPEN", "headRefOid": "abc",
                "commits": {{"nodes": [{{"commit": {{
                    "pushedDate": null,
                    "status": {{"context": {coderabbit}}},
                    "statusCheckRollup": {{"state": "SUCCESS", "contexts": {{"nodes": [{contexts}]}}}}
                }}}}]}}
            }}}}}}}}"#
        )
    }

    const CODERABBIT_PAUSED: &str =
        r#"{"state": "SUCCESS", "description": "Review paused", "targetUrl": null}"#;

    #[test]
    fn test_parse_coderabbit_status_beyond_first_page() {
        // The rollup page (100 contexts) didn't include CodeRabbit's status.
        let ci = r#"{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED",
                     "conclusion": "SUCCESS", "detailsUrl": null}"#;
        let (checks, _) = parse_checks_response(&checks_response(ci, CODERABBIT_PAUSED)).unwrap();
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[1].name, "CodeRabbit");
        assert_eq!(checks[1].status, CheckStatus::Success);
        assert_eq!(checks[1].detail.as_deref(), Some("Review paused"));
    }

    #[test]
    fn test_parse_coderabbit_status_not_duplicated() {
        let on_page = r#"{"__typename": "StatusContext", "context": "CodeRabbit",
                          "state": "SUCCESS", "targetUrl": null, "description": "Review paused"}"#;
        let (checks, _) =
            parse_checks_response(&checks_response(on_page, CODERABBIT_PAUSED)).unwrap();
        assert_eq!(checks.len(), 1);
        // No CodeRabbit status on the commit at all.
        let (checks, _) = parse_checks_response(&checks_response("", "null")).unwrap();
        assert!(checks.is_empty());
    }

    #[test]
    fn test_parse_check_details() {
        let data = parse_pr_response(fixture_full_response()).unwrap();
        // A check run's title, or a commit status's description; absent fields parse as None.
        assert_eq!(data.checks[0].detail.as_deref(), Some("2 tests failed"));
        assert_eq!(data.checks[1].detail, None);
        assert_eq!(data.checks[2].detail.as_deref(), Some("Deploying preview"));
    }

    #[test]
    fn test_parse_pushed_at() {
        let data = parse_pr_response(fixture_full_response()).unwrap();
        assert_eq!(data.pushed_at.as_deref(), Some("2026-03-31T09:30:00Z"));
    }

    #[test]
    fn test_parse_reviews() {
        let data = parse_pr_response(fixture_full_response()).unwrap();
        assert_eq!(data.reviews.len(), 2);

        assert_eq!(data.reviews[0].author, "copilot-pull-request-reviewer");
        assert_eq!(data.reviews[0].state, ReviewState::Commented);
        assert_eq!(data.reviews[0].commit_oid.as_deref(), Some("oldsha111"));

        assert_eq!(data.reviews[1].author, "human-reviewer");
        assert_eq!(data.reviews[1].state, ReviewState::Approved);
        assert_eq!(data.reviews[1].commit_oid.as_deref(), Some("abc123def"));
    }

    #[test]
    fn test_parse_requested_reviewers() {
        let data = parse_pr_response(fixture_full_response()).unwrap();
        // Bot and User logins are captured; the Team request (no login) is skipped.
        assert_eq!(data.requested_reviewers, vec!["copilot-pull-request-reviewer", "alice"]);
    }

    #[test]
    fn test_parse_merged_pr() {
        let json = r#"{
  "data": {
    "repository": {
      "pullRequest": {
        "number": 1,
        "state": "MERGED",
        "headRefOid": "def456",
        "reviewThreads": { "nodes": [] },
        "commits": { "nodes": [] }
      }
    }
  }
}"#;
        let data = parse_pr_response(json).unwrap();
        assert_eq!(data.state, PrState::Merged);
        assert!(data.threads.is_empty());
        assert!(data.checks.is_empty());
    }

    #[test]
    fn test_parse_graphql_errors() {
        let json = r#"{"data": null, "errors": [{"message": "Not found"}]}"#;
        let err = parse_pr_response(json).unwrap_err();
        assert!(err.to_string().contains("Not found"));
    }
}
