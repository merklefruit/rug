use crate::config::Config;
use crate::state::State;
use crate::types::*;

/// Filter threads to only unresolved ones, optionally filtered by review_bots config.
fn relevant_threads<'a>(threads: &'a [Thread], config: &Config) -> Vec<&'a Thread> {
    threads
        .iter()
        .filter(|t| !t.is_resolved)
        .filter(|t| {
            match &config.review_bots {
                None => true, // All comments are actionable
                Some(bots) => bots.iter().any(|b| b == &t.first_comment.author),
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
    if data.state == PrState::Merged {
        return (
            Verdict::Merged,
            vec![],
            SummaryOutput {
                total_unresolved: 0,
                new_since_last: 0,
                addressed: 0,
            },
        );
    }
    if data.state == PrState::Closed {
        return (
            Verdict::Closed,
            vec![],
            SummaryOutput {
                total_unresolved: 0,
                new_since_last: 0,
                addressed: 0,
            },
        );
    }

    let relevant = relevant_threads(&data.threads, config);
    let total_unresolved = relevant.len();

    let addressed_count = relevant
        .iter()
        .filter(|t| state.is_addressed(t.first_comment.id))
        .count();

    let new_threads: Vec<_> = relevant
        .iter()
        .filter(|t| !state.is_addressed(t.first_comment.id))
        .collect();

    let new_comments: Vec<CommentOutput> = new_threads
        .iter()
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
                .map(|r| ReplyOutput {
                    author: r.author.clone(),
                    body: r.body.clone(),
                })
                .collect(),
        })
        .collect();

    let summary = SummaryOutput {
        total_unresolved,
        new_since_last: new_comments.len(),
        addressed: addressed_count,
    };

    // Check CI status
    let all_settled = data.checks.iter().all(|c| c.status.is_terminal());
    let any_failing = data.checks.iter().any(|c| c.status == CheckStatus::Failure);
    let ci_pending = !all_settled;

    let verdict = if ci_pending {
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

/// Build CiOutput from checks.
pub fn build_ci_output(checks: &[Check]) -> CiOutput {
    let all_settled = checks.iter().all(|c| c.status.is_terminal());
    let any_failing = checks.iter().any(|c| c.status == CheckStatus::Failure);

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
        Check {
            name: name.to_string(),
            status,
            url: None,
        }
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
            pushed_at: None,
        };
        let config = Config {
            review_bots: Some(vec!["devin-ai[bot]".to_string()]),
            ..Config::default()
        };
        let (_, comments, summary) = compute(&data, &State::default(), &config);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].author, "devin-ai[bot]");
        assert_eq!(summary.total_unresolved, 1);
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
}
