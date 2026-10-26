#![doc = include_str!("../CRATE_DOCS.md")]
#![forbid(unsafe_code)]
#![warn(rust_2018_idioms)]
#![warn(rustdoc::broken_intra_doc_links)]

mod config;
mod github;
mod pr;
mod state;
mod types;
mod verdict;

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use config::Config;
use state::State;
use types::*;

#[derive(Parser)]
#[command(name = "rug", version, about = "Review Until Green — compact PR state for coding agents")]
struct Cli {
    /// PR URL, owner/repo#number, or omit for current branch
    pr: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Full PR status: new comments + CI + verdict
    Status,
    /// CI/check status only (lightweight poll)
    Checks,
    /// Block until CI checks settle, then print full status
    Watch {
        /// Timeout in seconds (default: 600)
        #[arg(long, default_value = "600")]
        timeout: u64,
        /// Settle window override in seconds (default: from rug.toml or 60)
        #[arg(long)]
        settle: Option<u64>,
    },
    /// Mark comment IDs as addressed in local state
    MarkAddressed {
        /// Comment IDs to mark as addressed
        #[arg(required = true)]
        ids: Vec<u64>,
    },
    /// Re-request a code review from GitHub Copilot
    RequestCopilotReview,
    /// Ask CodeRabbit to review the latest push (posts `@coderabbitai review`)
    RequestCoderabbitReview {
        /// Review the whole PR from scratch (`@coderabbitai full review`)
        #[arg(long)]
        full: bool,
        /// Post the request even if one was already made for the current HEAD
        #[arg(long)]
        force: bool,
    },
    /// Clear local state for a PR
    Reset,
}

fn cmd_status(pr_ref: &PrRef) -> Result<()> {
    let output = build_status_output(pr_ref)?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn build_status_output(pr_ref: &PrRef) -> Result<StatusOutput> {
    let config = Config::load(&std::env::current_dir()?)?;
    let rug_dir = state::rug_dir()?;
    let st = State::load(&rug_dir, &pr_ref.state_key())?;
    let data = github::fetch_pr_data(pr_ref)?;

    let (verdict_val, new_comments, summary) = verdict::compute(&data, &st, &config);
    let ci = verdict::build_ci_output(&data.checks);
    let reviews = verdict::build_reviews_output(&data.reviews, &data.head_sha);
    let coderabbit = verdict::build_coderabbit_output(&data, &config);

    Ok(StatusOutput {
        pr: PrInfoOutput {
            number: data.number,
            repo: data.repo,
            state: data.state.as_str().to_string(),
            head_sha: data.head_sha,
        },
        verdict: verdict_val.as_str().to_string(),
        ci,
        new_comments,
        reviews,
        requested_reviewers: data.requested_reviewers,
        coderabbit,
        summary,
    })
}

fn cmd_checks(pr_ref: &PrRef) -> Result<()> {
    let (checks, _pushed_at) = github::fetch_checks(pr_ref)?;
    let output = verdict::build_ci_output(&checks);
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn cmd_watch(pr_ref: &PrRef, timeout_secs: u64, settle_override: Option<u64>) -> Result<()> {
    let config = Config::load(&std::env::current_dir()?)?;
    let settle_secs = settle_override.unwrap_or(config.settle_window);
    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();

    // Check if there are already actionable comments — return immediately if so
    let initial = build_status_output(pr_ref)?;
    if !initial.new_comments.is_empty() {
        eprintln!(
            "{} unresolved comment(s) already present, returning immediately",
            initial.new_comments.len()
        );
        println!("{}", serde_json::to_string_pretty(&initial)?);
        return Ok(());
    }

    loop {
        wait_for_checks(pr_ref, start, timeout)?;

        // Settle window — wait for review bots to post
        if settle_secs > 0 {
            eprintln!("checks settled, waiting {settle_secs}s for review bots...");
            let settle_duration = Duration::from_secs(settle_secs);
            let remaining = timeout.saturating_sub(start.elapsed());
            std::thread::sleep(settle_duration.min(remaining));
        }

        // Return full status, unless a check started during the settle window (e.g. CodeRabbit
        // picking up a review request) and there is nothing to fix yet — then keep waiting
        // while time is left
        let output = build_status_output(pr_ref)?;
        if output.ci.all_settled || !output.new_comments.is_empty() || start.elapsed() >= timeout {
            println!("{}", serde_json::to_string_pretty(&output)?);
            return Ok(());
        }
        eprintln!("a check started during the settle window, waiting again...");
    }
}

/// Poll CI checks until they all settle. Fails once `timeout` has passed since `start`.
fn wait_for_checks(pr_ref: &PrRef, start: Instant, timeout: Duration) -> Result<()> {
    let poll_interval = Duration::from_secs(20);
    loop {
        let (checks, _) = github::fetch_checks(pr_ref)?;
        let ci = verdict::build_ci_output(&checks);

        let pending: Vec<_> = ci
            .checks
            .iter()
            .filter(|c| c.status == "in_progress" || c.status == "queued")
            .map(|c| c.name.as_str())
            .collect();
        eprintln!(
            "waiting for checks to settle... ({}/{} complete, {}s elapsed)",
            ci.checks.len() - pending.len(),
            ci.checks.len(),
            start.elapsed().as_secs()
        );

        if ci.all_settled {
            return Ok(());
        }

        if start.elapsed() + poll_interval > timeout {
            bail!(
                "Timed out after {}s waiting for checks to settle (still pending: {})",
                timeout.as_secs(),
                pending.join(", ")
            );
        }
        std::thread::sleep(poll_interval);
    }
}

fn cmd_mark_addressed(pr_ref: &PrRef, ids: &[u64]) -> Result<()> {
    let rug_dir = state::rug_dir()?;
    let key = pr_ref.state_key();
    let mut st = State::load(&rug_dir, &key)?;
    st.mark_addressed(ids);
    st.save(&rug_dir, &key)?;
    eprintln!("Marked {} comment(s) as addressed.", ids.len());
    Ok(())
}

fn cmd_request_copilot_review(pr_ref: &PrRef) -> Result<()> {
    github::request_copilot_review(pr_ref)?;
    eprintln!("Requested Copilot review on PR #{}.", pr_ref.number);
    let output = serde_json::json!({
        "requested_reviewer": "copilot",
        "pr": pr_ref.number,
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn cmd_request_coderabbit_review(pr_ref: &PrRef, full: bool, force: bool) -> Result<()> {
    let rug_dir = state::rug_dir()?;
    let key = pr_ref.state_key();
    let mut st = State::load(&rug_dir, &key)?;
    let head_sha = github::fetch_head_sha(pr_ref)?;

    // GitHub shows no pending request for CodeRabbit, so remember which HEAD was asked about:
    // asking again before a new push would only add noise to the PR.
    let requested = force || !st.coderabbit_requested(&head_sha);
    if requested {
        // Record the request before posting so a posted comment always has its guard; undo the
        // record if posting fails.
        let previous = st.coderabbit_requested_for.clone();
        st.mark_coderabbit_requested(&head_sha);
        st.save(&rug_dir, &key)?;
        if let Err(err) = github::request_coderabbit_review(pr_ref, full) {
            st.coderabbit_requested_for = previous;
            st.save(&rug_dir, &key)?;
            return Err(err);
        }
        eprintln!("Requested CodeRabbit review on PR #{}.", pr_ref.number);

        // Best effort: the request is already posted, so don't fail the command over this
        eprintln!("waiting for CodeRabbit to start reviewing...");
        match wait_for_coderabbit_pickup(pr_ref) {
            Ok(true) => eprintln!("CodeRabbit is reviewing."),
            Ok(false) => eprintln!(
                "CodeRabbit hasn't started reviewing after a minute; it may have declined the \
                 request."
            ),
            Err(err) => eprintln!("Couldn't check whether CodeRabbit started reviewing: {err:#}"),
        }
    } else {
        eprintln!(
            "CodeRabbit review already requested for HEAD {head_sha} on PR #{}; \
             pass --force to request again.",
            pr_ref.number
        );
    }

    let output = serde_json::json!({
        "requested_reviewer": "coderabbit",
        "pr": pr_ref.number,
        "head_sha": head_sha,
        "requested": requested,
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Wait up to a minute for CodeRabbit to start a requested review, returning whether it did.
/// CodeRabbit marks its status "Review in progress" about 10s after the comment; once it has,
/// a following `watch` waits for the review instead of returning before it starts.
fn wait_for_coderabbit_pickup(pr_ref: &PrRef) -> Result<bool> {
    let timeout = Duration::from_secs(60);
    let poll_interval = Duration::from_secs(5);
    let start = Instant::now();
    loop {
        let (checks, _) = github::fetch_checks(pr_ref)?;
        if verdict::coderabbit_reviewing(&checks) {
            return Ok(true);
        }
        if start.elapsed() + poll_interval > timeout {
            return Ok(false);
        }
        std::thread::sleep(poll_interval);
    }
}

fn cmd_reset(pr_ref: &PrRef) -> Result<()> {
    let rug_dir = state::rug_dir()?;
    State::delete(&rug_dir, &pr_ref.state_key())?;
    eprintln!("State cleared for PR #{}", pr_ref.number);
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let pr_ref = pr::resolve_pr(cli.pr.as_deref())
        .context("Specify a PR URL or run from a branch with an open PR")?;

    match cli.command {
        Commands::Status => cmd_status(&pr_ref),
        Commands::Checks => cmd_checks(&pr_ref),
        Commands::Watch { timeout, settle } => cmd_watch(&pr_ref, timeout, settle),
        Commands::MarkAddressed { ids } => cmd_mark_addressed(&pr_ref, &ids),
        Commands::RequestCopilotReview => cmd_request_copilot_review(&pr_ref),
        Commands::RequestCoderabbitReview { full, force } => {
            cmd_request_coderabbit_review(&pr_ref, full, force)
        }
        Commands::Reset => cmd_reset(&pr_ref),
    }
}
