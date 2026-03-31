//! Type definitions for `rug`.
//!
//! Organized into three groups:
//! - **GraphQL response types** — deserialized from GitHub API responses
//! - **Domain types** — internal representation used across modules
//! - **Output types** — serialized to JSON for stdout

use serde::{Deserialize, Serialize};

// ── GraphQL response types (deserialization only) ──
//
// These structs are populated by serde, not constructed in Rust code.
// Fields may appear unused but are required for correct deserialization.

/// Top-level GraphQL response envelope.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct GraphQLResponse {
    pub data: Option<GraphQLData>,
    pub errors: Option<Vec<GraphQLError>>,
}

/// A GraphQL error returned by the GitHub API.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct GraphQLError {
    pub message: String,
}

/// Root `data` field of the GraphQL response.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct GraphQLData {
    pub repository: Repository,
}

/// Repository node from the GraphQL response.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct Repository {
    #[serde(rename = "pullRequest")]
    pub pull_request: Option<PullRequestGql>,
}

/// Pull request node from the GraphQL response.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct PullRequestGql {
    pub number: u64,
    pub state: String,
    #[serde(rename = "headRefOid")]
    pub head_ref_oid: String,
    #[serde(rename = "reviewThreads")]
    pub review_threads: Option<Connection<ReviewThreadGql>>,
    pub commits: Connection<CommitNodeGql>,
}

/// Generic GraphQL connection (paginated list of nodes).
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct Connection<T> {
    pub nodes: Vec<T>,
}

/// A review thread on a pull request.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct ReviewThreadGql {
    #[serde(rename = "isResolved")]
    pub is_resolved: bool,
    pub comments: Connection<ReviewCommentGql>,
}

/// A single review comment within a thread.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct ReviewCommentGql {
    #[serde(rename = "databaseId")]
    pub database_id: Option<u64>,
    pub author: Option<AuthorGql>,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    #[serde(rename = "diffHunk")]
    pub diff_hunk: Option<String>,
    pub url: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// Author info from the GraphQL response.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct AuthorGql {
    pub login: String,
}

/// Commit node wrapper from the GraphQL response.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct CommitNodeGql {
    pub commit: CommitGql,
}

/// Commit data including push timestamp and check status.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct CommitGql {
    #[serde(rename = "pushedDate")]
    pub pushed_date: Option<String>,
    #[serde(rename = "statusCheckRollup")]
    pub status_check_rollup: Option<StatusCheckRollupGql>,
}

/// Aggregated status check rollup for a commit.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct StatusCheckRollupGql {
    pub state: String,
    pub contexts: Connection<CheckContextGql>,
}

/// A check context — either a GitHub Actions check run or a commit status.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(tag = "__typename")]
pub enum CheckContextGql {
    CheckRun {
        name: String,
        status: String,
        conclusion: Option<String>,
        #[serde(rename = "detailsUrl")]
        details_url: Option<String>,
    },
    StatusContext {
        context: String,
        state: String,
        #[serde(rename = "targetUrl")]
        target_url: Option<String>,
    },
}

// ── Domain types (internal) ──

/// A parsed reference to a GitHub pull request.
#[derive(Debug, Clone)]
pub struct PrRef {
    /// Repository owner (user or org).
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// PR number.
    pub number: u64,
}

impl PrRef {
    /// Returns a filesystem-safe key for local state storage.
    pub fn state_key(&self) -> String {
        format!("{}-{}-{}", self.owner, self.repo, self.number)
    }
}

/// Full PR data fetched from GitHub, ready for verdict computation.
#[derive(Debug, Clone)]
pub struct PrData {
    /// PR number.
    pub number: u64,
    /// Full repo identifier (e.g. "owner/repo").
    pub repo: String,
    /// Current PR state (open, merged, closed).
    pub state: PrState,
    /// HEAD commit SHA.
    pub head_sha: String,
    /// All review threads on the PR.
    pub threads: Vec<Thread>,
    /// CI check runs and status contexts.
    pub checks: Vec<Check>,
    /// When the HEAD commit was pushed (ISO 8601). Reserved for future use.
    #[allow(dead_code)]
    pub pushed_at: Option<String>,
}

/// PR lifecycle state.
#[derive(Debug, Clone, PartialEq)]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

impl PrState {
    /// Convert from GitHub GraphQL state string.
    pub fn from_gql(s: &str) -> Self {
        match s {
            "MERGED" => PrState::Merged,
            "CLOSED" => PrState::Closed,
            _ => PrState::Open,
        }
    }

    /// Returns the lowercase string representation for JSON output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }
}

/// A review thread containing a root comment and optional replies.
#[derive(Debug, Clone)]
pub struct Thread {
    /// Whether this thread has been resolved on GitHub.
    pub is_resolved: bool,
    /// The root comment that started this thread.
    pub first_comment: Comment,
    /// Subsequent replies in the thread.
    pub replies: Vec<Reply>,
}

/// A review comment on a specific file and line.
#[derive(Debug, Clone)]
pub struct Comment {
    /// GitHub database ID (used for tracking in local state).
    pub id: u64,
    /// Comment author's GitHub login.
    pub author: String,
    /// File path the comment refers to.
    pub path: Option<String>,
    /// Line number in the file.
    pub line: Option<u32>,
    /// Comment body text.
    pub body: String,
    /// Diff hunk context around the commented line.
    pub diff_hunk: Option<String>,
    /// URL to the comment on GitHub.
    pub url: String,
}

/// A reply within a review thread.
#[derive(Debug, Clone)]
pub struct Reply {
    /// Reply author's GitHub login.
    pub author: String,
    /// Reply body text.
    pub body: String,
}

/// A CI check run or commit status context.
#[derive(Debug, Clone)]
pub struct Check {
    /// Check name (e.g. "tests", "lint").
    pub name: String,
    /// Current status of this check.
    pub status: CheckStatus,
    /// URL to the check details page.
    pub url: Option<String>,
}

/// Status of a CI check.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckStatus {
    Queued,
    InProgress,
    Success,
    Failure,
    Neutral,
    Skipped,
}

impl CheckStatus {
    /// Returns true if this check has finished (won't change state).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Success | Self::Failure | Self::Neutral | Self::Skipped)
    }

    /// Returns the lowercase string representation for JSON output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::InProgress => "in_progress",
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Neutral => "neutral",
            Self::Skipped => "skipped",
        }
    }
}

/// Overall verdict for a PR's readiness.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// All checks passing, no unresolved comments.
    Approved,
    /// Unresolved review comments exist.
    ChangesRequested,
    /// CI checks are failing, but no unresolved comments.
    CiFailing,
    /// Checks are still running.
    Pending,
    /// PR has been merged.
    Merged,
    /// PR has been closed without merging.
    Closed,
}

impl Verdict {
    /// Returns the lowercase string representation for JSON output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes_requested",
            Self::CiFailing => "ci_failing",
            Self::Pending => "pending",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }
}

// ── Output types (JSON serialization) ──

/// JSON output for `rug status`.
#[derive(Serialize)]
pub struct StatusOutput {
    /// Basic PR info.
    pub pr: PrInfoOutput,
    /// Overall verdict string.
    pub verdict: String,
    /// CI check status.
    pub ci: CiOutput,
    /// Review comments not yet addressed.
    pub new_comments: Vec<CommentOutput>,
    /// Summary counts.
    pub summary: SummaryOutput,
}

/// PR identification info in JSON output.
#[derive(Serialize)]
pub struct PrInfoOutput {
    pub number: u64,
    pub repo: String,
    pub state: String,
    pub head_sha: String,
}

/// CI status in JSON output.
#[derive(Serialize)]
pub struct CiOutput {
    /// Rollup status: "passing", "failing", or "pending".
    pub status: String,
    /// True when all checks have reached a terminal state.
    pub all_settled: bool,
    /// Individual check results.
    pub checks: Vec<CheckOutput>,
}

/// A single check result in JSON output.
#[derive(Serialize)]
pub struct CheckOutput {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A review comment in JSON output.
#[derive(Serialize)]
pub struct CommentOutput {
    /// Database ID for use with `rug mark-addressed`.
    pub id: u64,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_hunk: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<ReplyOutput>,
}

/// A reply in JSON output.
#[derive(Serialize)]
pub struct ReplyOutput {
    pub author: String,
    pub body: String,
}

/// Comment count summary in JSON output.
#[derive(Serialize)]
pub struct SummaryOutput {
    /// Total unresolved comments matching config filters.
    pub total_unresolved: usize,
    /// Comments not yet in the addressed set.
    pub new_since_last: usize,
    /// Comments already marked as addressed.
    pub addressed: usize,
}
