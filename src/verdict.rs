//! Verdict computation and comment filtering.
//!
//! Takes PR data from GitHub, filters by addressed state and config,
//! and computes whether the PR is approved, needs changes, or is pending.

use crate::{config::Config, state::State, types::*};

/// Whether two GitHub logins name the same account. Logins are case-insensitive, and the GraphQL
/// API reports bots without the `[bot]` suffix that REST and the web UI show (`coderabbitai` vs
/// `coderabbitai[bot]`), so either form matches.
fn same_login(a: &str, b: &str) -> bool {
    fn base(login: &str) -> &str {
        login.strip_suffix("[bot]").unwrap_or(login)
    }
    base(a).eq_ignore_ascii_case(base(b))
}

/// Filter threads to only unresolved ones, optionally filtered by review_bots config.
fn relevant_threads<'a>(threads: &'a [Thread], config: &Config) -> Vec<&'a Thread> {
    threads
        .iter()
        .filter(|t| !t.is_resolved)
        .filter(|t| {
            match &config.review_bots {
                None => true, // All comments are actionable
                Some(bots) => bots.iter().any(|b| same_login(b, &t.first_comment.author)),
            }
        })
        .collect()
}

/// Compute the verdict given PR data, local state, and config.
/// Returns (verdict, new_comments, summary).
pub fn compute(
    data: &PrData,
    state: &State,
    config: &Config,
) -> (Verdict, Vec<CommentOutput>, SummaryOutput) {
    // Terminal states
    if matches!(data.state, PrState::Merged | PrState::Closed) {
        let verdict = if data.state == PrState::Merged { Verdict::Merged } else { Verdict::Closed };
        let empty = SummaryOutput { total_unresolved: 0, new_since_last: 0, addressed: 0 };
        return (verdict, vec![], empty);
    }

    let relevant = relevant_threads(&data.threads, config);
    let total_unresolved = relevant.len();

    let mut addressed_count = 0;
    let new_comments: Vec<CommentOutput> = relevant
        .iter()
        .filter(|t| {
            if state.is_addressed(t.first_comment.id) {
                addressed_count += 1;
                false
            } else {
                true
            }
        })
        .map(|t| CommentOutput {
            id: t.first_comment.id,
            author: t.first_comment.author.clone(),
            path: t.first_comment.path.clone(),
            line: t.first_comment.line,
            body: t.first_comment.body.clone(),
            diff_hunk: t.first_comment.diff_hunk.clone(),
            url: t.first_comment.url.clone(),
            replies: t
                .replies
                .iter()
                .map(|r| ReplyOutput { author: r.author.clone(), body: r.body.clone() })
                .collect(),
        })
        .collect();

    let summary = SummaryOutput {
        total_unresolved,
        new_since_last: new_comments.len(),
        addressed: addressed_count,
    };

    // CI status — single pass
    let (all_settled, any_failing) =
        data.checks.iter().fold((true, false), |(settled, failing), c| {
            (settled && c.status.is_terminal(), failing || c.status == CheckStatus::Failure)
        });

    let verdict = if !all_settled {
        Verdict::Pending
    } else if !new_comments.is_empty() {
        Verdict::ChangesRequested
    } else if any_failing {
        Verdict::CiFailing
    } else {
        Verdict::Approved
    };

    (verdict, new_comments, summary)
}

/// Build review outputs, flagging each review as stale when it was submitted
/// against a commit other than the current HEAD (i.e. the reviewer has not seen
/// the latest push).
pub fn build_reviews_output(reviews: &[Review], head_sha: &str) -> Vec<ReviewOutput> {
    reviews
        .iter()
        .map(|r| ReviewOutput {
            author: r.author.clone(),
            state: r.state.as_str().to_string(),
            stale: r.commit_oid.as_deref() != Some(head_sha),
        })
        .collect()
}

/// Build CiOutput from checks.
pub fn build_ci_output(checks: &[Check]) -> CiOutput {
    let (all_settled, any_failing) = checks.iter().fold((true, false), |(settled, failing), c| {
        (settled && c.status.is_terminal(), failing || c.status == CheckStatus::Failure)
    });

    let status = if !all_settled {
        "pending"
    } else if any_failing {
        "failing"
    } else {
        "passing"
    }
    .to_string();

    CiOutput {
        status,
        all_settled,
        checks: checks
            .iter()
            .map(|c| CheckOutput {
                name: c.name.clone(),
                status: c.status.as_str().to_string(),
                url: c.url.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_thread(id: u64, author: &str, resolved: bool) -> Thread {
        Thread {
            is_resolved: resolved,
            first_comment: Comment {
                id,
                author: author.to_string(),
                path: Some("src/main.rs".to_string()),
                line: Some(10),
                body: "Fix this".to_string(),
                diff_hunk: None,
                url: "https://example.com".to_string(),
            },
            replies: vec![],
        }
    }

    fn make_check(name: &str, status: CheckStatus) -> Check {
        Check { name: name.to_string(), status, url: None }
    }

    #[test]
    fn test_verdict_approved() {
        let data = PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Open,
            head_sha: "abc".to_string(),
            threads: vec![make_thread(1, "bot", true)], // resolved
            checks: vec![make_check("ci", CheckStatus::Success)],
            reviews: vec![],
            requested_reviewers: vec![],
            pushed_at: None,
        };
        let (verdict, comments, summary) = compute(&data, &State::default(), &Config::default());
        assert_eq!(verdict, Verdict::Approved);
        assert!(comments.is_empty());
        assert_eq!(summary.total_unresolved, 0);
    }

    #[test]
    fn test_verdict_changes_requested() {
        let data = PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Open,
            head_sha: "abc".to_string(),
            threads: vec![make_thread(1, "bot", false)],
            checks: vec![make_check("ci", CheckStatus::Success)],
            reviews: vec![],
            requested_reviewers: vec![],
            pushed_at: None,
        };
        let (verdict, comments, summary) = compute(&data, &State::default(), &Config::default());
        assert_eq!(verdict, Verdict::ChangesRequested);
        assert_eq!(comments.len(), 1);
        assert_eq!(summary.total_unresolved, 1);
        assert_eq!(summary.new_since_last, 1);
    }

    #[test]
    fn test_verdict_ci_failing() {
        let data = PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Open,
            head_sha: "abc".to_string(),
            threads: vec![],
            checks: vec![make_check("ci", CheckStatus::Failure)],
            reviews: vec![],
            requested_reviewers: vec![],
            pushed_at: None,
        };
        let (verdict, _, _) = compute(&data, &State::default(), &Config::default());
        assert_eq!(verdict, Verdict::CiFailing);
    }

    #[test]
    fn test_verdict_pending() {
        let data = PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Open,
            head_sha: "abc".to_string(),
            threads: vec![],
            checks: vec![make_check("ci", CheckStatus::InProgress)],
            reviews: vec![],
            requested_reviewers: vec![],
            pushed_at: None,
        };
        let (verdict, _, _) = compute(&data, &State::default(), &Config::default());
        assert_eq!(verdict, Verdict::Pending);
    }

    #[test]
    fn test_verdict_merged() {
        let data = PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Merged,
            head_sha: "abc".to_string(),
            threads: vec![make_thread(1, "bot", false)],
            checks: vec![],
            reviews: vec![],
            requested_reviewers: vec![],
            pushed_at: None,
        };
        let (verdict, _, _) = compute(&data, &State::default(), &Config::default());
        assert_eq!(verdict, Verdict::Merged);
    }

    #[test]
    fn test_delta_filtering() {
        let data = PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Open,
            head_sha: "abc".to_string(),
            threads: vec![
                make_thread(1, "bot", false),
                make_thread(2, "bot", false),
                make_thread(3, "bot", false),
            ],
            checks: vec![make_check("ci", CheckStatus::Success)],
            reviews: vec![],
            requested_reviewers: vec![],
            pushed_at: None,
        };
        let mut state = State::default();
        state.mark_addressed(&[1, 2]);

        let (verdict, comments, summary) = compute(&data, &state, &Config::default());
        assert_eq!(verdict, Verdict::ChangesRequested);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].id, 3);
        assert_eq!(summary.total_unresolved, 3);
        assert_eq!(summary.addressed, 2);
        assert_eq!(summary.new_since_last, 1);
    }

    #[test]
    fn test_all_addressed_is_approved() {
        let data = PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Open,
            head_sha: "abc".to_string(),
            threads: vec![make_thread(1, "bot", false)],
            checks: vec![make_check("ci", CheckStatus::Success)],
            reviews: vec![],
            requested_reviewers: vec![],
            pushed_at: None,
        };
        let mut state = State::default();
        state.mark_addressed(&[1]);

        let (verdict, comments, _) = compute(&data, &state, &Config::default());
        assert_eq!(verdict, Verdict::Approved);
        assert!(comments.is_empty());
    }

    #[test]
    fn test_review_bots_filter() {
        let data = PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Open,
            head_sha: "abc".to_string(),
            threads: vec![
                make_thread(1, "devin-ai[bot]", false),
                make_thread(2, "human-reviewer", false),
            ],
            checks: vec![make_check("ci", CheckStatus::Success)],
            reviews: vec![],
            requested_reviewers: vec![],
            pushed_at: None,
        };
        let config =
            Config { review_bots: Some(vec!["devin-ai[bot]".to_string()]), ..Config::default() };
        let (_, comments, summary) = compute(&data, &State::default(), &config);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].author, "devin-ai[bot]");
        assert_eq!(summary.total_unresolved, 1);
    }

    #[test]
    fn test_review_bots_filter_matches_graphql_bot_logins() {
        // GraphQL reports bot authors without the `[bot]` suffix that GitHub's UI shows.
        let data = PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Open,
            head_sha: "abc".to_string(),
            threads: vec![
                make_thread(1, "coderabbitai", false),
                make_thread(2, "cursor", false),
                make_thread(3, "human-reviewer", false),
            ],
            checks: vec![make_check("ci", CheckStatus::Success)],
            reviews: vec![],
            requested_reviewers: vec![],
            pushed_at: None,
        };
        let config = Config {
            review_bots: Some(vec!["coderabbitai[bot]".to_string(), "Cursor".to_string()]),
            ..Config::default()
        };
        let (_, comments, _) = compute(&data, &State::default(), &config);
        let ids: Vec<_> = comments.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn test_same_login() {
        assert!(same_login("coderabbitai", "coderabbitai[bot]"));
        assert!(same_login("CodeRabbitAI[bot]", "coderabbitai"));
        assert!(same_login("alice", "Alice"));
        assert!(!same_login("coderabbitai", "coderabbit"));
        assert!(!same_login("alice[bot]x", "alice"));
    }

    #[test]
    fn test_ci_output_passing() {
        let checks = vec![
            make_check("tests", CheckStatus::Success),
            make_check("lint", CheckStatus::Success),
        ];
        let ci = build_ci_output(&checks);
        assert_eq!(ci.status, "passing");
        assert!(ci.all_settled);
    }

    #[test]
    fn test_ci_output_pending() {
        let checks = vec![
            make_check("tests", CheckStatus::Success),
            make_check("lint", CheckStatus::InProgress),
        ];
        let ci = build_ci_output(&checks);
        assert_eq!(ci.status, "pending");
        assert!(!ci.all_settled);
    }

    fn make_review(author: &str, state: ReviewState, commit_oid: Option<&str>) -> Review {
        Review { author: author.to_string(), state, commit_oid: commit_oid.map(String::from) }
    }

    #[test]
    fn test_reviews_output_stale_detection() {
        let reviews = vec![
            // Reviewed the current HEAD — fresh.
            make_review("copilot-pull-request-reviewer", ReviewState::Commented, Some("head")),
            // Reviewed an older commit — stale.
            make_review("human", ReviewState::Approved, Some("old")),
            // Unknown commit — treated as stale.
            make_review("ghost", ReviewState::Commented, None),
        ];
        let out = build_reviews_output(&reviews, "head");

        assert_eq!(out.len(), 3);
        assert_eq!(out[0].author, "copilot-pull-request-reviewer");
        assert_eq!(out[0].state, "commented");
        assert!(!out[0].stale);
        assert_eq!(out[1].state, "approved");
        assert!(out[1].stale);
        assert!(out[2].stale);
    }

    #[test]
    fn test_reviews_output_empty() {
        assert!(build_reviews_output(&[], "head").is_empty());
    }
}
