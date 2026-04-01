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
    let poll_interval = Duration::from_secs(20);
    let start = Instant::now();

    // Poll until all checks settle
    loop {
        if start.elapsed() > timeout {
            bail!("Timed out after {}s waiting for checks to settle", timeout_secs);
        }

        let (checks, _) = github::fetch_checks(pr_ref)?;
        let ci = verdict::build_ci_output(&checks);

        let total = ci.checks.len();
        let elapsed = start.elapsed().as_secs();
        eprintln!(
            "waiting for checks to settle... ({}/{} complete, {}s elapsed)",
            ci.checks.iter().filter(|c| c.status != "in_progress" && c.status != "queued").count(),
            total,
            elapsed
        );

        if ci.all_settled {
            break;
        }

        // Check timeout before sleeping
        if start.elapsed() + poll_interval > timeout {
            bail!("Timed out after {}s waiting for checks to settle", timeout_secs);
        }
        std::thread::sleep(poll_interval);
    }

    // Settle window — wait for review bots to post
    if settle_secs > 0 {
        eprintln!("checks settled, waiting {}s for review bots...", settle_secs);
        let settle_duration = Duration::from_secs(settle_secs);
        let remaining = timeout.saturating_sub(start.elapsed());
        std::thread::sleep(settle_duration.min(remaining));
    }

    // Return full status
    let output = build_status_output(pr_ref)?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
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
        Commands::Reset => cmd_reset(&pr_ref),
    }
}
