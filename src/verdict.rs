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

/// Whether the review_bots config makes comments by `login` actionable.
fn is_tracked_author(config: &Config, login: &str) -> bool {
    match &config.review_bots {
        None => true, // All comments are actionable
        Some(bots) => bots.iter().any(|b| same_login(b, login)),
    }
}

/// Filter threads to only unresolved ones, optionally filtered by review_bots config.
fn relevant_threads<'a>(threads: &'a [Thread], config: &Config) -> Vec<&'a Thread> {
    threads
        .iter()
        .filter(|t| !t.is_resolved)
        .filter(|t| is_tracked_author(config, &t.first_comment.author))
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

/// Build CodeRabbit's progress on HEAD, or `None` if CodeRabbit is not a reviewer here: it has not
/// reviewed this PR, or review_bots leaves it out (so its comments are ignored anyway).
///
/// CodeRabbit's status on HEAD is the signal. Review freshness is only the fallback when there
/// is no status it recognizes, and a weak one: CodeRabbit can finish a commit without submitting
/// a new GitHub review (nothing new to say), and its chat replies in review threads are recorded
/// as reviews on HEAD.
pub fn build_coderabbit_output(data: &PrData, config: &Config) -> Option<CodeRabbitOutput> {
    if !is_tracked_author(config, CODERABBIT_LOGIN) {
        return None;
    }
    let review = data.reviews.iter().find(|r| same_login(&r.author, CODERABBIT_LOGIN))?;
    let check = data.checks.iter().find(|c| c.name.eq_ignore_ascii_case(CODERABBIT_STATUS));

    let state = check.and_then(coderabbit_check_state).unwrap_or_else(|| {
        if review.commit_oid.as_deref() == Some(data.head_sha.as_str()) {
            CodeRabbitState::Reviewed
        } else {
            CodeRabbitState::NotReviewed
        }
    });

    Some(CodeRabbitOutput {
        state: state.as_str().to_string(),
        detail: check.and_then(|c| c.detail.clone()),
    })
}

/// Read CodeRabbit's progress from its status on a commit, or `None` if the status text is not
/// one rug recognizes.
fn coderabbit_check_state(check: &Check) -> Option<CodeRabbitState> {
    if !check.status.is_terminal() {
        return Some(CodeRabbitState::Reviewing);
    }
    let detail = check.detail.as_deref().unwrap_or_default().to_ascii_lowercase();
    let state = if detail.contains("completed") || detail.contains("approved") {
        CodeRabbitState::Reviewed
    } else if detail.contains("paused") {
        CodeRabbitState::Paused
    } else if detail.contains("rate limit") {
        CodeRabbitState::RateLimited
    } else if detail.contains("skipped") {
        CodeRabbitState::Skipped
    } else if check.status == CheckStatus::Failure || detail.contains("fail") {
        CodeRabbitState::Failed
    } else {
        return None;
    };
    Some(state)
}

/// Whether CodeRabbit is reviewing the commit these checks belong to.
pub fn coderabbit_reviewing(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.name.eq_ignore_ascii_case(CODERABBIT_STATUS) && !c.status.is_terminal())
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
        Check { name: name.to_string(), status, url: None, detail: None }
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

    /// An open PR at HEAD `head` with the given reviews and checks.
    fn make_pr(reviews: Vec<Review>, checks: Vec<Check>) -> PrData {
        PrData {
            number: 1,
            repo: "owner/repo".to_string(),
            state: PrState::Open,
            head_sha: "head".to_string(),
            threads: vec![],
            checks,
            reviews,
            requested_reviewers: vec![],
            pushed_at: None,
        }
    }

    fn coderabbit_status(status: CheckStatus, description: &str) -> Check {
        Check { detail: Some(description.to_string()), ..make_check("CodeRabbit", status) }
    }

    /// CodeRabbit's state when its latest review is on `review_commit` and HEAD has `checks`.
    fn coderabbit_state(review_commit: &str, checks: Vec<Check>) -> Option<String> {
        let reviews =
            vec![make_review("coderabbitai", ReviewState::Commented, Some(review_commit))];
        build_coderabbit_output(&make_pr(reviews, checks), &Config::default()).map(|o| o.state)
    }

    #[test]
    fn test_coderabbit_absent_without_review() {
        // CodeRabbit posted a status (e.g. it only summarized the PR) but never reviewed it.
        let reviews = vec![make_review(
            "copilot-pull-request-reviewer",
            ReviewState::Commented,
            Some("head"),
        )];
        let checks = vec![coderabbit_status(CheckStatus::Success, "Review completed")];
        assert!(build_coderabbit_output(&make_pr(reviews, checks), &Config::default()).is_none());
    }

    #[test]
    fn test_coderabbit_reviewed() {
        // Latest review is older, but CodeRabbit finished HEAD with nothing new to post.
        let checks = vec![coderabbit_status(CheckStatus::Success, "Review completed")];
        assert_eq!(coderabbit_state("old", checks).as_deref(), Some("reviewed"));
        // CodeRabbit approved HEAD (automatic approvals).
        let checks = vec![coderabbit_status(CheckStatus::Success, "Review approved")];
        assert_eq!(coderabbit_state("old", checks).as_deref(), Some("reviewed"));
    }

    #[test]
    fn test_coderabbit_reviewing() {
        // In progress wins over a review on HEAD (e.g. a requested full review).
        let checks = vec![coderabbit_status(CheckStatus::InProgress, "Review in progress")];
        assert_eq!(coderabbit_state("head", checks).as_deref(), Some("reviewing"));
    }

    #[test]
    fn test_coderabbit_head_not_reviewed() {
        let cases = [
            (CheckStatus::Success, "Review paused", "paused"),
            (CheckStatus::Success, "Review skipped: incremental reviews are disabled", "skipped"),
            (CheckStatus::Success, "Review rate limited", "rate_limited"),
            (CheckStatus::Success, "Review failed", "failed"),
            (CheckStatus::Failure, "Review errored", "failed"),
        ];
        for (status, description, expected) in cases {
            let checks = vec![coderabbit_status(status, description)];
            assert_eq!(coderabbit_state("old", checks).as_deref(), Some(expected), "{description}");
        }
    }

    #[test]
    fn test_coderabbit_status_wins_over_review_on_head() {
        // CodeRabbit's chat replies in review threads are recorded as reviews on HEAD, so a
        // fresh-looking review doesn't override a status saying HEAD wasn't reviewed.
        let checks = vec![coderabbit_status(CheckStatus::Success, "Review paused")];
        assert_eq!(coderabbit_state("head", checks).as_deref(), Some("paused"));
    }

    #[test]
    fn test_coderabbit_falls_back_to_review_freshness() {
        // No status on HEAD (commit statuses disabled), or one rug doesn't recognize.
        assert_eq!(coderabbit_state("head", vec![]).as_deref(), Some("reviewed"));
        assert_eq!(coderabbit_state("old", vec![]).as_deref(), Some("not_reviewed"));
        let unknown = || vec![coderabbit_status(CheckStatus::Success, "Some future status")];
        assert_eq!(coderabbit_state("head", unknown()).as_deref(), Some("reviewed"));
        assert_eq!(coderabbit_state("old", unknown()).as_deref(), Some("not_reviewed"));
    }

    #[test]
    fn test_coderabbit_output_detail() {
        let reviews = vec![make_review("coderabbitai", ReviewState::Commented, Some("old"))];
        let checks = vec![
            make_check("ci", CheckStatus::Success),
            coderabbit_status(CheckStatus::Success, "Review paused"),
        ];
        let out = build_coderabbit_output(&make_pr(reviews, checks), &Config::default()).unwrap();
        assert_eq!(out.state, "paused");
        assert_eq!(out.detail.as_deref(), Some("Review paused"));

        // No status on HEAD: no detail.
        let reviews = vec![make_review("coderabbitai", ReviewState::Commented, Some("old"))];
        let out = build_coderabbit_output(&make_pr(reviews, vec![]), &Config::default()).unwrap();
        assert_eq!(out.detail, None);
    }

    #[test]
    fn test_coderabbit_respects_review_bots() {
        let pr = || {
            let reviews = vec![make_review("coderabbitai", ReviewState::Commented, Some("old"))];
            make_pr(reviews, vec![coderabbit_status(CheckStatus::Success, "Review paused")])
        };
        let only =
            |bot: &str| Config { review_bots: Some(vec![bot.to_string()]), ..Config::default() };
        // Left out of review_bots, its comments are ignored, so it isn't asked to re-review.
        assert!(build_coderabbit_output(&pr(), &only("cursor[bot]")).is_none());
        assert!(build_coderabbit_output(&pr(), &only("coderabbitai[bot]")).is_some());
    }

    #[test]
    fn test_coderabbit_reviewing_checks() {
        let pending = coderabbit_status(CheckStatus::InProgress, "Review in progress");
        assert!(coderabbit_reviewing(&[make_check("ci", CheckStatus::Success), pending]));
        let paused = coderabbit_status(CheckStatus::Success, "Review paused");
        assert!(!coderabbit_reviewing(&[paused]));
        // Another check in progress is not CodeRabbit reviewing.
        assert!(!coderabbit_reviewing(&[make_check("ci", CheckStatus::InProgress)]));
    }
}
