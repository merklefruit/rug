mod config;
mod github;
mod pr;
mod state;
mod types;
mod verdict;

use anyhow::{Context, Result};
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
    let config = Config::load(&std::env::current_dir()?)?;
    let rug_dir = state::rug_dir()?;
    let st = State::load(&rug_dir, &pr_ref.state_key())?;
    let data = github::fetch_pr_data(pr_ref)?;

    let (verdict_val, new_comments, summary) = verdict::compute(&data, &st, &config);
    let ci = verdict::build_ci_output(&data.checks);

    let output = StatusOutput {
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
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn cmd_checks(pr_ref: &PrRef) -> Result<()> {
    let (checks, _pushed_at) = github::fetch_checks(pr_ref)?;
    let output = verdict::build_ci_output(&checks);
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
        Commands::MarkAddressed { ids } => cmd_mark_addressed(&pr_ref, &ids),
        Commands::Reset => cmd_reset(&pr_ref),
    }
}
