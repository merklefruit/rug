use serde::{Deserialize, Serialize};

// ── GraphQL response types (deserialization only) ──

#[derive(Deserialize)]
pub struct GraphQLResponse {
    pub data: Option<GraphQLData>,
    pub errors: Option<Vec<GraphQLError>>,
}

#[derive(Deserialize)]
pub struct GraphQLError {
    pub message: String,
}

#[derive(Deserialize)]
pub struct GraphQLData {
    pub repository: Repository,
}

#[derive(Deserialize)]
pub struct Repository {
    #[serde(rename = "pullRequest")]
    pub pull_request: Option<PullRequestGql>,
}

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

#[derive(Deserialize)]
pub struct Connection<T> {
    pub nodes: Vec<T>,
}

#[derive(Deserialize)]
pub struct ReviewThreadGql {
    #[serde(rename = "isResolved")]
    pub is_resolved: bool,
    pub comments: Connection<ReviewCommentGql>,
}

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

#[derive(Deserialize)]
pub struct AuthorGql {
    pub login: String,
}

#[derive(Deserialize)]
pub struct CommitNodeGql {
    pub commit: CommitGql,
}

#[derive(Deserialize)]
pub struct CommitGql {
    #[serde(rename = "pushedDate")]
    pub pushed_date: Option<String>,
    #[serde(rename = "statusCheckRollup")]
    pub status_check_rollup: Option<StatusCheckRollupGql>,
}

#[derive(Deserialize)]
pub struct StatusCheckRollupGql {
    pub state: String,
    pub contexts: Connection<CheckContextGql>,
}

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

#[derive(Debug, Clone)]
pub struct PrRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

impl PrRef {
    pub fn state_key(&self) -> String {
        format!("{}-{}-{}", self.owner, self.repo, self.number)
    }
}

#[derive(Debug, Clone)]
pub struct PrData {
    pub number: u64,
    pub repo: String,
    pub state: PrState,
    pub head_sha: String,
    pub threads: Vec<Thread>,
    pub checks: Vec<Check>,
    pub pushed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

impl PrState {
    pub fn from_gql(s: &str) -> Self {
        match s {
            "MERGED" => PrState::Merged,
            "CLOSED" => PrState::Closed,
            _ => PrState::Open,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Thread {
    pub is_resolved: bool,
    pub first_comment: Comment,
    pub replies: Vec<Reply>,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub id: u64,
    pub author: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub body: String,
    pub diff_hunk: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub url: Option<String>,
}

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
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Success | Self::Failure | Self::Neutral | Self::Skipped)
    }

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

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Approved,
    ChangesRequested,
    CiFailing,
    Pending,
    Merged,
    Closed,
}

impl Verdict {
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

#[derive(Serialize)]
pub struct StatusOutput {
    pub pr: PrInfoOutput,
    pub verdict: String,
    pub ci: CiOutput,
    pub new_comments: Vec<CommentOutput>,
    pub summary: SummaryOutput,
}

#[derive(Serialize)]
pub struct PrInfoOutput {
    pub number: u64,
    pub repo: String,
    pub state: String,
    pub head_sha: String,
}

#[derive(Serialize)]
pub struct CiOutput {
    pub status: String,
    pub all_settled: bool,
    pub checks: Vec<CheckOutput>,
}

#[derive(Serialize)]
pub struct CheckOutput {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Serialize)]
pub struct CommentOutput {
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

#[derive(Serialize)]
pub struct ReplyOutput {
    pub author: String,
    pub body: String,
}

#[derive(Serialize)]
pub struct SummaryOutput {
    pub total_unresolved: usize,
    pub new_since_last: usize,
    pub addressed: usize,
}

#[derive(Serialize)]
pub struct ChecksOutput {
    pub status: String,
    pub all_settled: bool,
    pub checks: Vec<CheckOutput>,
}
